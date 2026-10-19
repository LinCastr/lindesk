//! Noise XX handshake and the encrypted datagram session that follows.
//!
//! There is no shared secret: each computer has a long-term X25519 identity
//! and the handshake exchanges them, mutually authenticated by possession of
//! the private keys. Who may connect is a human decision on the host (accept
//! dialog, or "always allow this computer" which pins the client's public
//! key); the client in turn pins the host's key on first use, so a later
//! impostor is detected. Both sides can show a short verification code
//! derived from the handshake for the paranoid.
//!
//! Wire framing: `0x48 step noise-bytes` for the three handshake messages,
//! `0x54 counter(8, LE) ciphertext` for transport.

use anyhow::{Context, Result, ensure};
use snow::{Builder, HandshakeState, StatelessTransportState};

use crate::Msg;

const PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const HS: u8 = 0x48;
const TR: u8 = 0x54;

/// Long-term key pair of this computer.
#[derive(Clone)]
pub struct Identity {
    pub private: [u8; 32],
    pub public: [u8; 32],
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn from_hex32(s: &str) -> Result<[u8; 32]> {
    let s = s.trim();
    ensure!(s.len() == 64, "expected 64 hex characters");
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)?;
    }
    Ok(out)
}

impl Identity {
    pub fn generate() -> Self {
        let kp = Builder::new(PATTERN.parse().expect("pattern")).generate_keypair().expect("keypair");
        Self { private: kp.private.try_into().expect("32 bytes"), public: kp.public.try_into().expect("32 bytes") }
    }

    /// Text form for the identity file: private key then public key, 128 hex characters.
    /// Store it with owner-only permissions.
    pub fn to_hex(&self) -> String {
        format!("{}{}", to_hex(&self.private), to_hex(&self.public))
    }

    pub fn from_hex(s: &str) -> Result<Self> {
        let s = s.trim();
        ensure!(s.len() == 128, "identity file must hold 128 hex characters");
        Ok(Self { private: from_hex32(&s[..64])?, public: from_hex32(&s[64..])? })
    }
}

/// Short, readable form of a public key for lists and dialogs: `a1b2-c3d4-e5f6-0718`.
pub fn fingerprint(public: &[u8; 32]) -> String {
    let h = to_hex(&public[..8]);
    format!("{}-{}-{}-{}", &h[0..4], &h[4..8], &h[8..12], &h[12..16])
}

pub fn key_hex(public: &[u8; 32]) -> String {
    to_hex(public)
}

fn frame(step: u8, noise: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(2 + noise.len());
    v.push(HS);
    v.push(step);
    v.extend_from_slice(noise);
    v
}

fn unframe(step: u8, d: &[u8]) -> Result<&[u8]> {
    ensure!(d.len() >= 2 && d[0] == HS && d[1] == step, "unexpected handshake message");
    Ok(&d[2..])
}

/// True for a handshake datagram, with its step number.
pub fn handshake_step(d: &[u8]) -> Option<u8> {
    (d.len() >= 2 && d[0] == HS).then_some(d[1])
}

pub fn is_transport(d: &[u8]) -> bool {
    d.first() == Some(&TR)
}

fn finish(hs: HandshakeState) -> Result<Session> {
    ensure!(hs.is_handshake_finished(), "handshake incomplete");
    let remote: [u8; 32] = hs.get_remote_static().context("peer sent no identity")?.try_into().context("bad key length")?;
    let hash = hs.get_handshake_hash().to_vec();
    Ok(Session { st: hs.into_stateless_transport_mode()?, next: 0, top: 0, seen: 0, any: false, remote, hash })
}

/// Client side: `new` yields message 1; `step2` consumes the host's reply and
/// yields message 3 plus the ready session.
pub struct Initiator {
    hs: HandshakeState,
}

impl Initiator {
    pub fn new(id: &Identity) -> Result<(Self, Vec<u8>)> {
        let mut hs = Builder::new(PATTERN.parse()?).local_private_key(&id.private)?.build_initiator()?;
        let mut buf = [0u8; 128];
        let n = hs.write_message(&[], &mut buf)?;
        Ok((Self { hs }, frame(1, &buf[..n])))
    }

    pub fn step2(mut self, dgram: &[u8]) -> Result<(Session, Vec<u8>)> {
        let mut sink = [0u8; 256];
        self.hs.read_message(unframe(2, dgram)?, &mut sink)?;
        let mut buf = [0u8; 256];
        let n = self.hs.write_message(&[], &mut buf)?;
        Ok((finish(self.hs)?, frame(3, &buf[..n])))
    }
}

/// Host side: `step1` consumes message 1 and yields message 2; `step3`
/// consumes message 3 and yields the session.
pub struct Responder {
    hs: HandshakeState,
}

impl Responder {
    pub fn new(id: &Identity) -> Result<Self> {
        Ok(Self { hs: Builder::new(PATTERN.parse()?).local_private_key(&id.private)?.build_responder()? })
    }

    pub fn step1(&mut self, dgram: &[u8]) -> Result<Vec<u8>> {
        let mut sink = [0u8; 256];
        self.hs.read_message(unframe(1, dgram)?, &mut sink)?;
        let mut buf = [0u8; 256];
        let n = self.hs.write_message(&[], &mut buf)?;
        Ok(frame(2, &buf[..n]))
    }

    pub fn step3(mut self, dgram: &[u8]) -> Result<Session> {
        let mut sink = [0u8; 256];
        self.hs.read_message(unframe(3, dgram)?, &mut sink)?;
        finish(self.hs)
    }
}

/// Encrypted datagram channel with replay protection.
pub struct Session {
    st: StatelessTransportState,
    next: u64,
    // replay window: highest counter seen + bitmap of the 128 before it
    top: u64,
    seen: u128,
    any: bool,
    remote: [u8; 32],
    hash: Vec<u8>,
}

impl Session {
    /// The peer's long-term public key, authenticated by the handshake.
    pub fn remote_static(&self) -> [u8; 32] {
        self.remote
    }

    /// Four digits both sides can read aloud to confirm nobody is in the middle.
    pub fn verification_code(&self) -> String {
        let n = u32::from_le_bytes([self.hash[0], self.hash[1], self.hash[2], self.hash[3]]);
        format!("{:04}", n % 10_000)
    }

    pub fn seal(&mut self, msg: &Msg) -> Vec<u8> {
        let plain = msg.encode();
        let counter = self.next;
        self.next += 1;
        let mut out = vec![0u8; 9 + plain.len() + 16];
        out[0] = TR;
        out[1..9].copy_from_slice(&counter.to_le_bytes());
        let n = self.st.write_message(counter, &plain, &mut out[9..]).expect("encrypt");
        out.truncate(9 + n);
        out
    }

    /// Verifies and decodes a transport datagram. Rejects forgeries and replays.
    pub fn open(&mut self, data: &[u8]) -> Result<Msg> {
        ensure!(data.len() > 9 + 16 && data[0] == TR, "not a transport datagram");
        let counter = u64::from_le_bytes(data[1..9].try_into().unwrap());
        let mut plain = vec![0u8; data.len()];
        let n = self
            .st
            .read_message(counter, &data[9..], &mut plain)
            .map_err(|_| anyhow::anyhow!("authentication failed"))?;
        // Only update the window after authentication succeeded.
        if self.any {
            if counter > self.top {
                let shift = counter - self.top;
                self.seen = if shift >= 128 { 0 } else { (self.seen << shift) | (1u128 << (shift - 1)) };
                self.top = counter;
            } else {
                let back = self.top - counter;
                ensure!(back != 0 && back <= 128, "replayed or too old");
                let bit = 1u128 << (back - 1);
                ensure!(self.seen & bit == 0, "replayed");
                self.seen |= bit;
            }
        } else {
            self.any = true;
            self.top = counter;
        }
        Msg::decode(&plain[..n])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connect() -> (Session, Session, Identity, Identity) {
        let (c, h) = (Identity::generate(), Identity::generate());
        let (init, m1) = Initiator::new(&c).unwrap();
        let mut resp = Responder::new(&h).unwrap();
        let m2 = resp.step1(&m1).unwrap();
        let (cs, m3) = init.step2(&m2).unwrap();
        let hs = resp.step3(&m3).unwrap();
        (cs, hs, c, h)
    }

    #[test]
    fn handshake_exchanges_identities() {
        let (cs, hs, c, h) = connect();
        assert_eq!(hs.remote_static(), c.public);
        assert_eq!(cs.remote_static(), h.public);
        assert_eq!(cs.verification_code(), hs.verification_code());
    }

    #[test]
    fn identity_survives_hex_roundtrip() {
        let id = Identity::generate();
        let back = Identity::from_hex(&id.to_hex()).unwrap();
        assert_eq!((back.private, back.public), (id.private, id.public));
    }

    #[test]
    fn roundtrip_both_directions_and_replay() {
        let (mut cs, mut hs, ..) = connect();
        let d = cs.seal(&Msg::Ping(42));
        assert_eq!(hs.open(&d).unwrap(), Msg::Ping(42));
        assert!(hs.open(&d).is_err(), "replay must fail");
        let d = hs.seal(&Msg::Status(1));
        assert_eq!(cs.open(&d).unwrap(), Msg::Status(1));
    }

    #[test]
    fn rejects_tamper_and_foreign_session() {
        let (mut cs, mut hs, ..) = connect();
        let mut d = cs.seal(&Msg::Bye);
        let last = d.len() - 1;
        d[last] ^= 1;
        assert!(hs.open(&d).is_err());
        let (mut other_c, _other_h, ..) = connect();
        assert!(hs.open(&other_c.seal(&Msg::Bye)).is_err(), "another session's keys must not open");
    }

    #[test]
    fn out_of_order_within_window() {
        let (mut cs, mut hs, ..) = connect();
        let a = cs.seal(&Msg::Ping(1));
        let b = cs.seal(&Msg::Ping(2));
        assert_eq!(hs.open(&b).unwrap(), Msg::Ping(2));
        assert_eq!(hs.open(&a).unwrap(), Msg::Ping(1));
    }

    #[test]
    fn handshake_rejects_wrong_step() {
        let h = Identity::generate();
        let mut resp = Responder::new(&h).unwrap();
        assert!(resp.step1(&[0x48, 3, 0, 0]).is_err());
        assert!(resp.step1(&[1, 2, 3]).is_err());
    }
}
