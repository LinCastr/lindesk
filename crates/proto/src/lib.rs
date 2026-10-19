//! Wire protocol shared by host and client.
//!
//! A connection starts with a Noise XX handshake (see [`session`]) that gives
//! both sides each other's long-term public key and fresh session keys; after
//! that every datagram is `counter(8, LE) || ChaCha20-Poly1305(plaintext)`.
//! A sliding window on the receiver rejects replays.
//!
//! Plaintext is `type(1) || body`. Video travels as fragments of one H.264
//! access unit each; losing any fragment drops the whole frame (see
//! [`Reassembler`]) because retransmitting is slower than the next frame.

pub mod events;
pub mod id;
pub mod session;

pub use session::{Identity, Initiator, Responder, Session};

use std::collections::HashMap;

use anyhow::{Result, bail, ensure};
use reed_solomon_erasure::galois_8::ReedSolomon;

pub const VERSION: u16 = 3;
/// Payload bytes of one video fragment. Keeps the datagram under a 1280 MTU.
pub const FRAG_PAYLOAD: usize = 1100;
pub const MAX_DATAGRAM: usize = 1500;
pub const MAX_NAME: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputEvent {
    /// Absolute position, 0..=65535 across the captured screen.
    MouseAbs { x: u16, y: u16 },
    /// Linux evdev button code (BTN_LEFT = 0x110 ...).
    Button { code: u16, down: bool },
    /// Linux evdev key code. `state`: 0 up, 1 down, 2 repeat.
    Key { code: u16, state: u8 },
    Wheel { dx: i16, dy: i16 },
    /// Release every pressed key and button (sent on focus loss).
    ReleaseAll,
}

/// One datagram of video: a data or parity shard of an FEC group.
///
/// A frame (H.264 access unit) is cut into groups of at most [`GROUP_DATA`]
/// data shards; each group gets `m` Reed-Solomon parity shards, so any `k`
/// of its `k + m` shards rebuild it. Shard `idx < k` is data, the rest parity.
#[derive(Clone, Debug, PartialEq)]
pub struct Shard {
    pub frame: u32,
    pub key: bool,
    pub group: u8,
    pub groups: u8,
    pub idx: u8,
    pub k: u8,
    pub m: u8,
    /// Length of the whole access unit in bytes.
    pub len: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    /// First transport message: protocol version and a display name of the client computer.
    Hello { version: u16, name: String },
    Video(Shard),
    RequestKeyframe,
    /// Receive report for the last interval; drives the host's bitrate control.
    Stats { frames_ok: u32, frames_lost: u32 },
    Ping(u64),
    Pong(u64),
    /// `seq` lets the host drop the duplicate copies of a reliable event.
    Input { seq: u32, ev: InputEvent },
    Bye,
    /// Host's answer to Hello: 0 waiting for the owner, 1 accepted, 2 denied.
    Status(u8),
}

impl Msg {
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(32);
        match self {
            Msg::Hello { version, name } => {
                b.push(1);
                b.extend_from_slice(&version.to_le_bytes());
                b.extend_from_slice(&name.as_bytes()[..name.len().min(MAX_NAME)]);
            }
            Msg::Video(sh) => {
                b.push(2);
                b.extend_from_slice(&sh.frame.to_le_bytes());
                b.extend_from_slice(&[sh.key as u8, sh.group, sh.groups, sh.idx, sh.k, sh.m]);
                b.extend_from_slice(&sh.len.to_le_bytes());
                b.extend_from_slice(&sh.data);
            }
            Msg::RequestKeyframe => b.push(3),
            Msg::Stats { frames_ok, frames_lost } => {
                b.push(4);
                b.extend_from_slice(&frames_ok.to_le_bytes());
                b.extend_from_slice(&frames_lost.to_le_bytes());
            }
            Msg::Ping(t) => {
                b.push(5);
                b.extend_from_slice(&t.to_le_bytes());
            }
            Msg::Pong(t) => {
                b.push(6);
                b.extend_from_slice(&t.to_le_bytes());
            }
            Msg::Input { seq, ev } => {
                b.push(7);
                b.extend_from_slice(&seq.to_le_bytes());
                match ev {
                    InputEvent::MouseAbs { x, y } => {
                        b.push(0);
                        b.extend_from_slice(&x.to_le_bytes());
                        b.extend_from_slice(&y.to_le_bytes());
                    }
                    InputEvent::Button { code, down } => {
                        b.push(1);
                        b.extend_from_slice(&code.to_le_bytes());
                        b.push(*down as u8);
                    }
                    InputEvent::Key { code, state } => {
                        b.push(2);
                        b.extend_from_slice(&code.to_le_bytes());
                        b.push(*state);
                    }
                    InputEvent::Wheel { dx, dy } => {
                        b.push(3);
                        b.extend_from_slice(&dx.to_le_bytes());
                        b.extend_from_slice(&dy.to_le_bytes());
                    }
                    InputEvent::ReleaseAll => b.push(4),
                }
            }
            Msg::Bye => b.push(8),
            Msg::Status(v) => {
                b.push(9);
                b.push(*v);
            }
        }
        b
    }

    pub fn decode(p: &[u8]) -> Result<Msg> {
        ensure!(!p.is_empty(), "empty message");
        let u16_at = |i: usize| -> Result<u16> {
            Ok(u16::from_le_bytes(p.get(i..i + 2).ok_or_else(|| anyhow::anyhow!("short"))?.try_into()?))
        };
        let u32_at = |i: usize| -> Result<u32> {
            Ok(u32::from_le_bytes(p.get(i..i + 4).ok_or_else(|| anyhow::anyhow!("short"))?.try_into()?))
        };
        let u64_at = |i: usize| -> Result<u64> {
            Ok(u64::from_le_bytes(p.get(i..i + 8).ok_or_else(|| anyhow::anyhow!("short"))?.try_into()?))
        };
        let u8_at = |i: usize| -> Result<u8> { p.get(i).copied().ok_or_else(|| anyhow::anyhow!("short")) };
        Ok(match p[0] {
            1 => Msg::Hello {
                version: u16_at(1)?,
                name: String::from_utf8_lossy(p.get(3..).unwrap_or_default()).chars().filter(|c| !c.is_control()).take(MAX_NAME).collect(),
            },
            2 => {
                let (group, groups, idx, k, m) = (u8_at(6)?, u8_at(7)?, u8_at(8)?, u8_at(9)?, u8_at(10)?);
                ensure!(k > 0 && groups > 0 && group < groups, "bad group");
                ensure!((idx as usize) < k as usize + m as usize, "bad shard index");
                let data = p.get(15..).unwrap_or_default().to_vec();
                ensure!(data.len() == FRAG_PAYLOAD, "bad shard size");
                Msg::Video(Shard { frame: u32_at(1)?, key: u8_at(5)? != 0, group, groups, idx, k, m, len: u32_at(11)?, data })
            }
            3 => Msg::RequestKeyframe,
            4 => Msg::Stats { frames_ok: u32_at(1)?, frames_lost: u32_at(5)? },
            5 => Msg::Ping(u64_at(1)?),
            6 => Msg::Pong(u64_at(1)?),
            7 => {
                let seq = u32_at(1)?;
                let ev = match u8_at(5)? {
                    0 => InputEvent::MouseAbs { x: u16_at(6)?, y: u16_at(8)? },
                    1 => InputEvent::Button { code: u16_at(6)?, down: u8_at(8)? != 0 },
                    2 => InputEvent::Key { code: u16_at(6)?, state: u8_at(8)? },
                    3 => InputEvent::Wheel { dx: u16_at(6)? as i16, dy: u16_at(8)? as i16 },
                    4 => InputEvent::ReleaseAll,
                    k => bail!("unknown input kind {k}"),
                };
                Msg::Input { seq, ev }
            }
            8 => Msg::Bye,
            9 => Msg::Status(u8_at(1)?),
            t => bail!("unknown message type {t}"),
        })
    }
}

/// Most data shards in one FEC group (GF(2^8) allows 255 shards in total).
pub const GROUP_DATA: usize = 200;

/// Splits one access unit into FEC-protected shards.
/// `parity_permille` is parity shards per 1000 data shards (150 = 15 %); 0 disables FEC.
pub fn fragment(frame: u32, key: bool, au: &[u8], parity_permille: u32) -> Vec<Msg> {
    let total = au.len().div_ceil(FRAG_PAYLOAD).max(1);
    let groups = total.div_ceil(GROUP_DATA);
    let mut out = Vec::with_capacity(total + total / 6 + groups);
    for g in 0..groups {
        let lo = (g * GROUP_DATA * FRAG_PAYLOAD).min(au.len());
        let hi = ((g + 1) * GROUP_DATA * FRAG_PAYLOAD).min(au.len());
        let chunk = &au[lo..hi];
        let k = chunk.len().div_ceil(FRAG_PAYLOAD).max(1);
        let m = if parity_permille == 0 {
            0
        } else {
            ((k as u32 * parity_permille).div_ceil(1000).max(1) as usize).min(255 - k)
        };
        let mut shards: Vec<Vec<u8>> = (0..k + m)
            .map(|i| {
                let mut v = vec![0u8; FRAG_PAYLOAD];
                if i < k {
                    let part = &chunk[(i * FRAG_PAYLOAD).min(chunk.len())..((i + 1) * FRAG_PAYLOAD).min(chunk.len())];
                    v[..part.len()].copy_from_slice(part);
                }
                v
            })
            .collect();
        if m > 0 {
            ReedSolomon::new(k, m).expect("rs params").encode(&mut shards).expect("rs encode");
        }
        for (i, data) in shards.into_iter().enumerate() {
            out.push(Msg::Video(Shard {
                frame,
                key,
                group: g as u8,
                groups: groups as u8,
                idx: i as u8,
                k: k as u8,
                m: m as u8,
                len: au.len() as u32,
                data,
            }));
        }
    }
    out
}

/// A frame the decoder may consume.
pub struct Frame {
    pub key: bool,
    pub data: Vec<u8>,
}

struct Group {
    k: u8,
    m: u8,
    shards: Vec<Option<Vec<u8>>>,
    got: usize,
    done: Option<Vec<u8>>,
}

struct Partial {
    key: bool,
    len: u32,
    groups: Vec<Option<Group>>,
    ndone: usize,
}

impl Partial {
    fn new(sh: &Shard) -> Self {
        Self { key: sh.key, len: sh.len, groups: (0..sh.groups).map(|_| None).collect(), ndone: 0 }
    }

    /// Stores a shard; true once every group of the frame is rebuilt.
    fn add(&mut self, sh: Shard) -> Result<bool> {
        ensure!(sh.groups as usize == self.groups.len() && sh.len == self.len, "inconsistent shard");
        let g = self.groups[sh.group as usize].get_or_insert_with(|| Group {
            k: sh.k,
            m: sh.m,
            shards: vec![None; sh.k as usize + sh.m as usize],
            got: 0,
            done: None,
        });
        ensure!(g.k == sh.k && g.m == sh.m, "inconsistent group");
        if g.done.is_some() || g.shards[sh.idx as usize].is_some() {
            return Ok(self.ndone == self.groups.len());
        }
        g.shards[sh.idx as usize] = Some(sh.data);
        g.got += 1;
        if g.got >= g.k as usize {
            let k = g.k as usize;
            if g.shards[..k].iter().any(Option::is_none) {
                ReedSolomon::new(k, g.m as usize)?.reconstruct_data(&mut g.shards)?;
            }
            let mut bytes = Vec::with_capacity(k * FRAG_PAYLOAD);
            for s in &g.shards[..k] {
                bytes.extend_from_slice(s.as_ref().expect("data shard rebuilt"));
            }
            g.done = Some(bytes);
            self.ndone += 1;
        }
        Ok(self.ndone == self.groups.len())
    }

    fn assemble(self) -> Vec<u8> {
        let mut data: Vec<u8> = self.groups.into_iter().flatten().filter_map(|g| g.done).flatten().collect();
        data.truncate(self.len as usize);
        data
    }
}

/// Rebuilds frames with latency over completeness:
///
/// * a frame is delivered the moment enough shards arrive (parity is only
///   decoded when a data shard is actually missing);
/// * when a newer frame completes, every older incomplete frame is abandoned;
/// * after an unrecoverable loss, delta frames are withheld until the next
///   keyframe, because H.264 P-frames reference the lost data.
pub struct Reassembler {
    partial: HashMap<u32, Partial>,
    newest_done: Option<u32>,
    waiting_key: bool,
    pub ok: u32,
    pub lost: u32,
}

impl Default for Reassembler {
    fn default() -> Self {
        Self { partial: HashMap::new(), newest_done: None, waiting_key: true, ok: 0, lost: 0 }
    }
}

/// What the caller must do after feeding a shard.
#[derive(Default)]
pub struct Pushed {
    pub frame: Option<Frame>,
    /// Ask the host for a keyframe now.
    pub need_keyframe: bool,
}

fn newer(a: u32, b: u32) -> bool {
    a != b && a.wrapping_sub(b) < u32::MAX / 2
}

impl Reassembler {
    pub fn push(&mut self, sh: Shard) -> Pushed {
        let mut out = Pushed::default();
        let frame = sh.frame;
        if let Some(done) = self.newest_done {
            if !newer(frame, done) {
                return out; // late shard of a frame we already settled
            }
        }
        let p = self.partial.entry(frame).or_insert_with(|| Partial::new(&sh));
        if !p.add(sh).unwrap_or(false) {
            // Shards of a newer frame arriving while older ones are incomplete
            // means those older ones will not finish in time.
            self.abandon_older_than(frame, &mut out);
            return out;
        }
        let p = self.partial.remove(&frame).unwrap();
        self.abandon_older_than(frame, &mut out);
        self.partial.retain(|f, _| newer(*f, frame));
        let skipped = match self.newest_done {
            Some(d) => frame.wrapping_sub(d).saturating_sub(1),
            None => 0,
        };
        self.newest_done = Some(frame);
        if skipped > 0 && !p.key {
            // frames vanished entirely (no shard ever arrived)
            self.lost += skipped;
            self.waiting_key = true;
            out.need_keyframe = true;
        }
        if p.key {
            self.waiting_key = false;
        }
        if self.waiting_key {
            out.need_keyframe = true;
            return out;
        }
        let key = p.key;
        self.ok += 1;
        out.frame = Some(Frame { key, data: p.assemble() });
        out
    }

    fn abandon_older_than(&mut self, frame: u32, out: &mut Pushed) {
        let stale: Vec<u32> = self.partial.keys().copied().filter(|f| newer(frame, *f)).collect();
        if !stale.is_empty() {
            for f in &stale {
                self.partial.remove(f);
            }
            self.lost += stale.len() as u32;
            self.waiting_key = true;
            out.need_keyframe = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msg_roundtrip() {
        for m in [
            Msg::Hello { version: 1, name: "PC de Teste".into() },
            Msg::Status(1),
            Msg::RequestKeyframe,
            Msg::Stats { frames_ok: 5, frames_lost: 2 },
            Msg::Input { seq: 9, ev: InputEvent::MouseAbs { x: 1, y: 65535 } },
            Msg::Input { seq: 9, ev: InputEvent::Wheel { dx: -3, dy: 7 } },
            Msg::Input { seq: 1, ev: InputEvent::Key { code: 30, state: 2 } },
            Msg::Video(Shard { frame: 3, key: true, group: 0, groups: 1, idx: 1, k: 2, m: 1, len: 5, data: vec![7; FRAG_PAYLOAD] }),
        ] {
            assert_eq!(Msg::decode(&m.encode()).unwrap(), m);
        }
    }

    fn feed(r: &mut Reassembler, frame: u32, key: bool, au: &[u8], fec: u32, skip: &[usize]) -> Vec<Pushed> {
        let mut res = Vec::new();
        for (i, m) in fragment(frame, key, au, fec).into_iter().enumerate() {
            if skip.contains(&i) {
                continue;
            }
            if let Msg::Video(sh) = m {
                res.push(r.push(sh));
            }
        }
        res
    }

    #[test]
    fn delivers_in_order() {
        let mut r = Reassembler::default();
        let au: Vec<u8> = (0..3000u32).map(|x| x as u8).collect();
        let out = feed(&mut r, 0, true, &au, 150, &[]);
        assert_eq!(out.iter().find_map(|p| p.frame.as_ref()).unwrap().data, au);
        assert_eq!(feed(&mut r, 1, false, &au, 150, &[]).iter().filter(|p| p.frame.is_some()).count(), 1);
        assert_eq!((r.ok, r.lost), (2, 0));
    }

    #[test]
    fn fec_recovers_lost_shards() {
        let mut r = Reassembler::default();
        let au: Vec<u8> = (0..60_000u32).map(|x| (x * 7) as u8).collect(); // 55 data shards
        let out = feed(&mut r, 0, true, &au, 150, &[3, 20, 40]); // 3 of 55 data lost, 9 parity
        assert_eq!(out.iter().find_map(|p| p.frame.as_ref()).unwrap().data, au);
        assert_eq!(r.lost, 0);
    }

    #[test]
    fn large_frame_uses_several_groups() {
        let mut r = Reassembler::default();
        let au: Vec<u8> = (0..300_000u32).map(|x| (x ^ (x >> 8)) as u8).collect(); // 273 shards -> 2 groups
        let out = feed(&mut r, 0, true, &au, 100, &[5, 250]);
        assert_eq!(out.iter().find_map(|p| p.frame.as_ref()).unwrap().data, au);
    }

    #[test]
    fn unrecoverable_loss_waits_for_keyframe() {
        let mut r = Reassembler::default();
        let au = vec![1u8; 3000]; // 3 data + 1 parity
        feed(&mut r, 0, true, &au, 150, &[]);
        feed(&mut r, 1, false, &au, 150, &[0, 1]); // 2 lost > 1 parity
        let out = feed(&mut r, 2, false, &au, 150, &[]);
        assert!(out.iter().any(|p| p.need_keyframe));
        assert!(out.iter().all(|p| p.frame.is_none()), "delta after loss must be withheld");
        let out = feed(&mut r, 3, true, &au, 150, &[]);
        assert!(out.iter().any(|p| p.frame.is_some()), "keyframe resumes");
        assert!(r.lost >= 1);
    }

    #[test]
    fn no_fec_mode() {
        let mut r = Reassembler::default();
        let au = vec![9u8; 2500];
        let out = feed(&mut r, 0, true, &au, 0, &[]);
        assert_eq!(out.iter().find_map(|p| p.frame.as_ref()).unwrap().data, au);
    }
}
