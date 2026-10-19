//! UDP socket, Noise handshake and the encrypted session, shared by the capture
//! thread (sending video) and the receive loop (control and input).

use std::net::{SocketAddr, UdpSocket};
use std::sync::Mutex;
use std::sync::atomic::AtomicU32;
use std::time::{Duration, Instant};

use anyhow::Result;
use lindesk_proto::session::{handshake_step, is_transport};
use lindesk_proto::{Identity, Msg, Responder, Session};
use socket2::{Domain, Socket, Type};

pub const DEFAULT_FEC: u32 = 150;
const STALE: Duration = Duration::from_secs(5);

struct Pending {
    from: SocketAddr,
    resp: Responder,
}

#[derive(Default)]
struct Inner {
    session: Option<Session>,
    peer: Option<SocketAddr>,
    hs: Option<Pending>,
    last_rx: Option<Instant>,
}

pub struct Net {
    pub sock: UdpSocket,
    identity: Identity,
    inner: Mutex<Inner>,
    /// FEC parity shards per 1000 data shards; raised when the client reports loss.
    pub fec_permille: AtomicU32,
    /// Frames sent since the last statistics report.
    pub frames_sent: AtomicU32,
}

/// What one received datagram turned out to be.
pub enum Rx {
    Nothing,
    /// A handshake finished: a new (not yet approved) client.
    Established,
    Msg(Msg, SocketAddr),
}

impl Net {
    pub fn bind(addr: SocketAddr, identity: Identity) -> Result<Self> {
        let s = Socket::new(Domain::for_address(addr), Type::DGRAM, None)?;
        // Big buffers absorb a keyframe burst without kernel drops.
        s.set_send_buffer_size(4 << 20)?;
        s.set_recv_buffer_size(1 << 20)?;
        s.bind(&addr.into())?;
        let sock: UdpSocket = s.into();
        sock.set_read_timeout(Some(Duration::from_millis(200)))?;
        Ok(Self {
            sock,
            identity,
            inner: Mutex::new(Inner::default()),
            fec_permille: AtomicU32::new(DEFAULT_FEC),
            frames_sent: AtomicU32::new(0),
        })
    }

    pub fn peer(&self) -> Option<SocketAddr> {
        self.inner.lock().unwrap().peer
    }

    pub fn remote_static(&self) -> Option<[u8; 32]> {
        self.inner.lock().unwrap().session.as_ref().map(Session::remote_static)
    }

    pub fn verification(&self) -> String {
        self.inner.lock().unwrap().session.as_ref().map(Session::verification_code).unwrap_or_default()
    }

    /// Seconds since the last authenticated packet from the client.
    pub fn idle(&self) -> Duration {
        self.inner.lock().unwrap().last_rx.map_or(Duration::ZERO, |t| t.elapsed())
    }

    pub fn drop_session(&self) {
        let mut i = self.inner.lock().unwrap();
        i.session = None;
        i.peer = None;
        i.hs = None;
        i.last_rx = None;
    }

    /// Reads one datagram (or times out) and advances the handshake / decrypts it.
    pub fn receive(&self, buf: &mut [u8]) -> Rx {
        let Ok((n, from)) = self.sock.recv_from(buf) else { return Rx::Nothing };
        let d = &buf[..n];
        let mut i = self.inner.lock().unwrap();
        match handshake_step(d) {
            Some(1) => {
                // One client at a time: ignore newcomers while the current one is alive.
                let busy = i.session.is_some() && i.peer != Some(from) && i.last_rx.is_some_and(|t| t.elapsed() < STALE);
                if busy {
                    return Rx::Nothing;
                }
                let Ok(mut resp) = Responder::new(&self.identity) else { return Rx::Nothing };
                let Ok(m2) = resp.step1(d) else { return Rx::Nothing };
                let _ = self.sock.send_to(&m2, from);
                i.hs = Some(Pending { from, resp });
                Rx::Nothing
            }
            Some(3) => {
                let Some(p) = i.hs.take_if(|p| p.from == from) else { return Rx::Nothing };
                match p.resp.step3(d) {
                    Ok(session) => {
                        i.session = Some(session);
                        i.peer = Some(from);
                        i.last_rx = Some(Instant::now());
                        Rx::Established
                    }
                    Err(_) => Rx::Nothing,
                }
            }
            Some(_) => Rx::Nothing,
            None if is_transport(d) => {
                let Some(s) = i.session.as_mut() else { return Rx::Nothing };
                match s.open(d) {
                    Ok(msg) => {
                        i.last_rx = Some(Instant::now());
                        // Authenticated packet: follow the peer if its address changed (NAT rebinding).
                        i.peer = Some(from);
                        Rx::Msg(msg, from)
                    }
                    Err(_) => Rx::Nothing, // forged/replayed: drop silently
                }
            }
            None => Rx::Nothing,
        }
    }

    pub fn send(&self, m: &Msg) {
        self.send_all(std::slice::from_ref(m));
    }

    pub fn send_all(&self, msgs: &[Msg]) {
        for m in msgs {
            let mut i = self.inner.lock().unwrap();
            let (Some(peer), Some(s)) = (i.peer, i.session.as_mut()) else { return };
            let d = s.seal(m);
            drop(i);
            if let Err(e) = self.sock.send_to(&d, peer) {
                log::debug!("send: {e}");
            }
        }
    }
}
