//! Network side of the client: handshake, approval wait, receive loop, stats.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use lindesk_proto::events::ViewerEvent;
use lindesk_proto::session::{fingerprint, handshake_step, key_hex};
use lindesk_proto::{Identity, InputEvent, Initiator, Msg, Reassembler, VERSION};
use socket2::{Domain, Socket, Type};

use crate::decode::Decode;

/// How long the host's owner has to answer (the host itself gives up at 60 s).
const APPROVAL_WAIT: Duration = Duration::from_secs(70);
const HANDSHAKE_WAIT: Duration = Duration::from_secs(12);
/// No video for this long means the host went away.
const VIDEO_SILENCE_US: u64 = 8_000_000;

pub struct Session {
    sock: UdpSocket,
    chan: Mutex<lindesk_proto::Session>,
    step3: Vec<u8>,
    name: String,
    seq: AtomicU32,
    pub rtt_us: AtomicU64,
    last_video_us: AtomicU64,
    pub stop: AtomicBool,
    start: Instant,
}

impl Session {
    /// Opens the socket and runs the Noise handshake with the host.
    /// `pinned` is the host key remembered from an earlier visit, if any.
    pub fn connect(host: SocketAddr, id: &Identity, name: String, pinned: Option<[u8; 32]>, emit: &dyn Fn(ViewerEvent)) -> Result<Arc<Self>> {
        let local: SocketAddr = if host.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }.parse()?;
        let s = Socket::new(Domain::for_address(local), Type::DGRAM, None)?;
        s.set_recv_buffer_size(4 << 20)?;
        s.bind(&local.into())?;
        let sock: UdpSocket = s.into();
        sock.connect(host)?;
        sock.set_read_timeout(Some(Duration::from_millis(300)))?;

        let (init, m1) = Initiator::new(id)?;
        let deadline = Instant::now() + HANDSHAKE_WAIT;
        let mut buf = vec![0u8; lindesk_proto::MAX_DATAGRAM];
        let (chan, step3) = loop {
            if Instant::now() > deadline {
                bail!("No answer from {host}. Is the other computer sharing, and is the network or VPN connected?");
            }
            sock.send(&m1)?;
            let Ok(n) = sock.recv(&mut buf) else { continue };
            if handshake_step(&buf[..n]) == Some(2) {
                match init.step2(&buf[..n]) {
                    Ok(r) => break r,
                    Err(e) => bail!("Handshake failed: {e}"),
                }
            }
        };
        let remote = chan.remote_static();
        emit(ViewerEvent::HostIdentity { key: key_hex(&remote), fingerprint: fingerprint(&remote) });
        if let Some(p) = pinned {
            if p != remote {
                bail!("The identity of this computer changed since your last connection. If it was reinstalled, remove it from your recent computers and connect again. Otherwise someone may be impersonating it.");
            }
        }
        Ok(Arc::new(Self {
            sock,
            chan: Mutex::new(chan),
            step3,
            name,
            seq: AtomicU32::new(1),
            rtt_us: AtomicU64::new(0),
            last_video_us: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            start: Instant::now(),
        }))
    }

    pub fn verification(&self) -> String {
        self.chan.lock().unwrap().verification_code()
    }

    pub fn send(&self, m: &Msg) {
        let d = self.chan.lock().unwrap().seal(m);
        let _ = self.sock.send(&d);
    }

    /// Sends Hello until the host answers, then waits for its owner to accept.
    pub fn wait_accept(&self, emit: &dyn Fn(ViewerEvent)) -> Result<()> {
        let deadline = Instant::now() + APPROVAL_WAIT;
        let mut buf = vec![0u8; lindesk_proto::MAX_DATAGRAM];
        let mut told = false;
        let mut last_hello: Option<Instant> = None;
        while Instant::now() < deadline {
            if last_hello.is_none_or(|t| t.elapsed() > Duration::from_millis(400)) {
                // Message 3 may have been lost; the host ignores duplicates.
                let _ = self.sock.send(&self.step3);
                self.send(&Msg::Hello { version: VERSION, name: self.name.clone() });
                last_hello = Some(Instant::now());
            }
            let Ok(n) = self.sock.recv(&mut buf) else { continue };
            let Ok(msg) = self.chan.lock().unwrap().open(&buf[..n]) else { continue };
            match msg {
                Msg::Status(0) if !told => {
                    told = true;
                    emit(ViewerEvent::Waiting { verification: self.verification() });
                }
                Msg::Status(1) => return Ok(()),
                Msg::Status(2) => {
                    emit(ViewerEvent::Denied);
                    bail!("The other computer did not accept the connection.");
                }
                _ => {}
            }
        }
        bail!("The other computer did not answer the request in time.")
    }

    /// Mouse motion goes out once (a lost sample is replaced by the next).
    /// Key/button events go out three times: stuck keys are worse than duplicates,
    /// and the host drops the copies by sequence number.
    pub fn input(&self, ev: InputEvent) {
        let reliable = !matches!(ev, InputEvent::MouseAbs { .. } | InputEvent::Wheel { .. });
        let seq = if reliable { self.seq.fetch_add(1, Ordering::Relaxed) } else { 0 };
        let m = Msg::Input { seq, ev };
        for _ in 0..if reliable { 3 } else { 1 } {
            self.send(&m);
        }
    }

    fn now_us(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }

    /// Spawns the receive and keepalive threads. Frames go to `decode`.
    pub fn run(self: &Arc<Self>, decode: Arc<Decode>, emit: Arc<dyn Fn(ViewerEvent) + Send + Sync>) {
        self.last_video_us.store(self.now_us(), Ordering::Relaxed);
        let me = self.clone();
        let emit_rx = emit.clone();
        let stats = Arc::new(Mutex::new((0u32, 0u32))); // (ok, lost) since last report
        let st = stats.clone();
        std::thread::Builder::new()
            .name("net-rx".into())
            .spawn(move || {
                let mut re = Reassembler::default();
                let mut buf = vec![0u8; lindesk_proto::MAX_DATAGRAM];
                let mut last_req = Instant::now() - Duration::from_secs(1);
                let (mut seen_ok, mut seen_lost) = (0u32, 0u32);
                let mut first = true;
                while !me.stop.load(Ordering::Relaxed) {
                    let Ok(n) = me.sock.recv(&mut buf) else { continue };
                    let Ok(msg) = me.chan.lock().unwrap().open(&buf[..n]) else { continue };
                    match msg {
                        Msg::Video(shard) => {
                            if std::mem::take(&mut first) {
                                emit_rx(ViewerEvent::Connected);
                            }
                            me.last_video_us.store(me.now_us(), Ordering::Relaxed);
                            let out = re.push(shard);
                            if let Some(f) = out.frame {
                                decode.push(f.data);
                            }
                            if out.need_keyframe && last_req.elapsed() > Duration::from_millis(100) {
                                last_req = Instant::now();
                                me.send(&Msg::RequestKeyframe);
                            }
                            let mut s = st.lock().unwrap();
                            s.0 += re.ok - seen_ok;
                            s.1 += re.lost - seen_lost;
                            seen_ok = re.ok;
                            seen_lost = re.lost;
                        }
                        Msg::Pong(t) => me.rtt_us.store(me.now_us().saturating_sub(t), Ordering::Relaxed),
                        _ => {}
                    }
                }
            })
            .expect("spawn net-rx");

        // Keepalive + stats report once per second; give up when the video stops.
        let me = self.clone();
        std::thread::Builder::new()
            .name("net-tick".into())
            .spawn(move || {
                while !me.stop.load(Ordering::Relaxed) {
                    if me.now_us().saturating_sub(me.last_video_us.load(Ordering::Relaxed)) > VIDEO_SILENCE_US {
                        me.stop.store(true, Ordering::Relaxed);
                        emit(ViewerEvent::Error { message: "Connection lost: the other computer stopped sending video.".into() });
                        return;
                    }
                    me.send(&Msg::Ping(me.now_us()));
                    let (ok, lost) = std::mem::take(&mut *stats.lock().unwrap());
                    me.send(&Msg::Stats { frames_ok: ok, frames_lost: lost });
                    emit(ViewerEvent::Stats { fps: ok, lost, rtt_ms: me.rtt_us.load(Ordering::Relaxed) as f32 / 1000.0 });
                    std::thread::sleep(Duration::from_secs(1));
                }
            })
            .expect("spawn net-tick");
    }

    pub fn bye(&self) {
        for _ in 0..3 {
            self.send(&Msg::Bye);
        }
        self.stop.store(true, Ordering::Relaxed);
    }
}
