//! Network side of the client: handshake, keepalive, receive loop, stats.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use lindesk_proto::events::ViewerEvent;
use lindesk_proto::{Channel, InputEvent, Msg, Reassembler, Side, VERSION};
use socket2::{Domain, Socket, Type};

use crate::decode::Decode;

pub struct Session {
    sock: UdpSocket,
    chan: Mutex<Channel>,
    seq: AtomicU32,
    pub connected: AtomicBool,
    pub rtt_us: AtomicU64,
    last_video_us: AtomicU64,
    pub stop: AtomicBool,
    start: Instant,
}

impl Session {
    pub fn connect(host: SocketAddr, key: &[u8; 32]) -> Result<Arc<Self>> {
        let local: SocketAddr = if host.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }.parse()?;
        let s = Socket::new(Domain::for_address(local), Type::DGRAM, None)?;
        s.set_recv_buffer_size(4 << 20)?;
        s.bind(&local.into())?;
        let sock: UdpSocket = s.into();
        sock.connect(host)?;
        sock.set_read_timeout(Some(Duration::from_millis(200)))?;
        Ok(Arc::new(Self {
            sock,
            chan: Mutex::new(Channel::new(key, Side::Client)),
            seq: AtomicU32::new(1),
            connected: AtomicBool::new(false),
            rtt_us: AtomicU64::new(0),
            last_video_us: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            start: Instant::now(),
        }))
    }

    pub fn send(&self, m: &Msg) {
        let d = self.chan.lock().unwrap().seal(m);
        let _ = self.sock.send(&d);
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
        // Receiver
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
                while !me.stop.load(Ordering::Relaxed) {
                    let n = match me.sock.recv(&mut buf) {
                        Ok(n) => n,
                        Err(_) => continue,
                    };
                    let Ok(msg) = me.chan.lock().unwrap().open(&buf[..n]) else { continue };
                    match msg {
                        Msg::Video(shard) => {
                            if !me.connected.swap(true, Ordering::Relaxed) {
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
                        Msg::Pong(t) => {
                            me.rtt_us.store(me.now_us().saturating_sub(t), Ordering::Relaxed);
                        }
                        _ => {}
                    }
                }
            })
            .expect("spawn net-rx");

        // Keepalive + stats report once per second; Hello until the host answers.
        let me = self.clone();
        std::thread::Builder::new()
            .name("net-tick".into())
            .spawn(move || {
                while !me.stop.load(Ordering::Relaxed) {
                    // Video silent for 3 s (host restarted or link died): handshake again.
                    if me.connected.load(Ordering::Relaxed)
                        && me.now_us().saturating_sub(me.last_video_us.load(Ordering::Relaxed)) > 3_000_000
                    {
                        emit(ViewerEvent::Reconnecting);
                        me.connected.store(false, Ordering::Relaxed);
                    }
                    if !me.connected.load(Ordering::Relaxed) {
                        me.send(&Msg::Hello { version: VERSION });
                        me.send(&Msg::Ping(me.now_us()));
                        std::thread::sleep(Duration::from_millis(300));
                        continue;
                    }
                    me.send(&Msg::Ping(me.now_us()));
                    let (ok, lost) = std::mem::take(&mut *stats.lock().unwrap());
                    me.send(&Msg::Stats { frames_ok: ok, frames_lost: lost });
                    emit(ViewerEvent::Stats {
                        fps: ok,
                        lost,
                        rtt_ms: me.rtt_us.load(Ordering::Relaxed) as f32 / 1000.0,
                    });
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
