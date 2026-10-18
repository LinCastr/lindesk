mod inject;
mod net;
mod pipeline;
pub mod portal;

pub use pipeline::Encoder;

use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use lindesk_proto::events::HostEvent;
use lindesk_proto::{Msg, VERSION};

use net::Net;
use pipeline::{Settings, Source, Streamer};

const SESSION_TIMEOUT: Duration = Duration::from_secs(5);
const MIN_KBPS: u32 = 2_000;


pub struct Options {
    pub listen: SocketAddr,
    pub key: [u8; 32],
    /// portal | test | node:<pipewire-node-id>
    pub source: String,
    pub encoder: Encoder,
    pub fps: u32,
    /// Maximum (and initial) bitrate in kbit/s; lowered automatically on loss.
    pub bitrate: u32,
    pub cursor: bool,
    pub input: bool,
}

/// Runs the host until `stop` is set. Blocks.
pub fn run(opts: Options, stop: Arc<AtomicBool>, emit: impl Fn(HostEvent)) -> Result<()> {
    let net = Arc::new(Net::bind(opts.listen, &opts.key)?);

    // Keep the portal session alive for the whole run; dropping it ends the capture.
    let mut _portal = None;
    let source = match opts.source.as_str() {
        "test" => Source::Test,
        "portal" => {
            let p = portal::open(opts.cursor)?;
            let fd = Arc::new(p.fd.try_clone()?);
            let node = p.node;
            _portal = Some(p);
            Source::Portal { fd, node }
        }
        s => Source::Node(s.strip_prefix("node:").and_then(|n| n.parse().ok()).context("source: portal | test | node:<id>")?),
    };

    let streamer = Streamer::new(
        net.clone(),
        source,
        Settings { encoder: opts.encoder, fps: opts.fps, kbps: opts.bitrate },
    )?;
    let mut injector = if opts.input {
        match inject::Injector::new() {
            Ok(i) => Some(i),
            Err(e) => {
                emit(HostEvent::Error { message: format!("Input control unavailable: {e:#}") });
                None
            }
        }
    } else {
        None
    };

    emit(HostEvent::Listening { addr: opts.listen.to_string(), encoder: format!("{:?}", streamer.encoder()) });
    let mut buf = vec![0u8; lindesk_proto::MAX_DATAGRAM];
    let mut last_rx = Instant::now();
    let mut last_stats = Instant::now();
    let mut kbps = opts.bitrate;
    let mut calm = 0u32;

    while !stop.load(Ordering::Relaxed) {
        match net.sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                let Ok(msg) = net.open(&buf[..n]) else { continue }; // forged/replayed: drop silently
                if matches!(msg, Msg::Bye) && net.peer().is_none() {
                    continue; // the client sends Bye several times; only the first counts
                }
                last_rx = Instant::now();
                // Authenticated packet: follow the peer even if its address changed.
                if net.peer() != Some(from) {
                    emit(HostEvent::ClientConnected { addr: from.to_string() });
                    net.set_peer(Some(from));
                }
                match msg {
                    Msg::Hello { version } => {
                        if version != VERSION {
                            emit(HostEvent::Error { message: format!("client protocol {version} != {VERSION}") });
                        }
                        if let Err(e) = streamer.start() {
                            emit(HostEvent::Error { message: format!("cannot start streaming: {e:#}") });
                        }
                        streamer.force_keyframe();
                    }
                    Msg::RequestKeyframe => streamer.force_keyframe(),
                    Msg::Ping(t) => net.send(&Msg::Pong(t)),
                    Msg::Stats { frames_ok, frames_lost } => {
                        // AIMD: cut hard on loss, creep back up when clean.
                        if frames_lost > 0 {
                            kbps = (kbps * 7 / 10).max(MIN_KBPS);
                            calm = 0;
                            let f = net.fec_permille.load(Ordering::Relaxed);
                            net.fec_permille.store((f + 100).min(500), Ordering::Relaxed);
                            streamer.set_bitrate(kbps);
                            log::info!("loss {frames_lost}/{}: bitrate -> {kbps} kbit/s", frames_ok + frames_lost);
                        } else if frames_ok > 0 {
                            calm += 1;
                            if calm >= 5 {
                                calm = 0;
                                let f = net.fec_permille.load(Ordering::Relaxed);
                                net.fec_permille.store(f.saturating_sub(50).max(net::DEFAULT_FEC), Ordering::Relaxed);
                                if kbps < opts.bitrate {
                                    kbps = (kbps * 108 / 100).min(opts.bitrate);
                                    streamer.set_bitrate(kbps);
                                }
                            }
                        }
                    }
                    Msg::Input { seq, ev } => {
                        if let Some(inj) = injector.as_mut() {
                            inj.apply(seq, ev);
                        }
                    }
                    Msg::Bye => {
                        disconnect(&streamer, &net, injector.as_mut());
                        emit(HostEvent::ClientLeft { reason: "disconnected".into() });
                    }
                    Msg::Video(_) | Msg::Pong(_) => {}
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => log::warn!("recv: {e}"),
        }
        if net.peer().is_some() && last_rx.elapsed() > SESSION_TIMEOUT {
            disconnect(&streamer, &net, injector.as_mut());
            emit(HostEvent::ClientLeft { reason: "timed out".into() });
            kbps = opts.bitrate;
        }
        if net.peer().is_some() && last_stats.elapsed() >= Duration::from_secs(1) {
            last_stats = Instant::now();
            emit(HostEvent::Stats {
                fps: net.frames_sent.swap(0, Ordering::Relaxed),
                kbps,
                fec_permille: net.fec_permille.load(Ordering::Relaxed),
            });
        }
    }
    disconnect(&streamer, &net, injector.as_mut());
    Ok(())
}

fn disconnect(streamer: &Streamer, net: &Net, inj: Option<&mut inject::Injector>) {
    streamer.stop();
    net.set_peer(None);
    if let Some(i) = inj {
        i.release_all();
    }
}
