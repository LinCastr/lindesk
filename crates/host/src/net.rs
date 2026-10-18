//! UDP socket plus the encrypted channel, shared by the capture thread
//! (sending video) and the receive loop (control and input).

use std::net::{SocketAddr, UdpSocket};
use std::sync::Mutex;
use std::sync::atomic::AtomicU32;

use anyhow::Result;
use lindesk_proto::{Channel, Msg, Side};
use socket2::{Domain, Socket, Type};

pub const DEFAULT_FEC: u32 = 150;

pub struct Net {
    pub sock: UdpSocket,
    chan: Mutex<Channel>,
    peer: Mutex<Option<SocketAddr>>,
    /// FEC parity shards per 1000 data shards; raised when the client reports loss.
    pub fec_permille: AtomicU32,
}

impl Net {
    pub fn bind(addr: SocketAddr, key: &[u8; 32]) -> Result<Self> {
        let s = Socket::new(Domain::for_address(addr), Type::DGRAM, None)?;
        // Big buffers absorb a keyframe burst without kernel drops.
        s.set_send_buffer_size(4 << 20)?;
        s.set_recv_buffer_size(1 << 20)?;
        s.bind(&addr.into())?;
        let sock: UdpSocket = s.into();
        sock.set_read_timeout(Some(std::time::Duration::from_millis(200)))?;
        Ok(Self { sock, chan: Mutex::new(Channel::new(key, Side::Host)), peer: Mutex::new(None), fec_permille: AtomicU32::new(DEFAULT_FEC) })
    }

    pub fn peer(&self) -> Option<SocketAddr> {
        *self.peer.lock().unwrap()
    }

    pub fn set_peer(&self, p: Option<SocketAddr>) {
        *self.peer.lock().unwrap() = p;
    }

    pub fn open(&self, data: &[u8]) -> Result<Msg> {
        self.chan.lock().unwrap().open(data)
    }

    pub fn send(&self, m: &Msg) {
        self.send_all(std::slice::from_ref(m));
    }

    pub fn send_all(&self, msgs: &[Msg]) {
        let Some(peer) = self.peer() else { return };
        for m in msgs {
            let d = self.chan.lock().unwrap().seal(m);
            if let Err(e) = self.sock.send_to(&d, peer) {
                log::debug!("send: {e}");
            }
        }
    }
}
