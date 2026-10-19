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
use std::collections::HashSet;
use std::sync::mpsc::Receiver;

use lindesk_proto::events::{HostCommand, HostEvent};
use lindesk_proto::session::{fingerprint, key_hex};
use lindesk_proto::{Identity, Msg, VERSION};

use net::{Net, Rx};
use pipeline::{Settings, Source, Streamer};

const SESSION_TIMEOUT: Duration = Duration::from_secs(5);
const MIN_KBPS: u32 = 2_000;


pub struct Options {
    pub listen: SocketAddr,
    pub identity: Identity,
    /// Public keys of computers that connect without asking.
    pub trusted: Vec<[u8; 32]>,
    /// Accept everybody (tests and headless use only).
    pub accept_all: bool,
    /// portal | test | node:<pipewire-node-id>
    pub source: String,
    pub encoder: Encoder,
    pub fps: u32,
    /// Maximum (and initial) bitrate in kbit/s; lowered automatically on loss.
    pub bitrate: u32,
    pub cursor: bool,
    pub input: bool,
}

const PENDING_TIMEOUT: Duration = Duration::from_secs(60);

enum State {
    /// No client, or a client that has not said hello yet.
    Idle,
    /// Waiting for the owner's answer.
    Pending { name: String, since: Instant, last_ping: Instant },
    Active,
}

/// Runs the host until `stop` is set. Blocks.
pub fn run(opts: Options, stop: Arc<AtomicBool>, commands: Receiver<HostCommand>, emit: impl Fn(HostEvent)) -> Result<()> {
    let net = Arc::new(Net::bind(opts.listen, opts.identity.clone())?);

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
    let mut trusted: HashSet<[u8; 32]> = opts.trusted.iter().copied().collect();
    let mut state = State::Idle;
    let mut buf = vec![0u8; lindesk_proto::MAX_DATAGRAM];
    let mut last_stats = Instant::now();
    let mut kbps = opts.bitrate;
    let mut calm = 0u32;

    // Ends the current client (if any) and goes back to waiting.
    let hang_up = |state: &mut State, inj: Option<&mut inject::Injector>, reason: &str| {
        if !matches!(state, State::Idle) {
            emit(HostEvent::ClientLeft { reason: reason.into() });
        }
        streamer.stop();
        net.drop_session();
        if let Some(i) = inj {
            i.release_all();
        }
        *state = State::Idle;
    };

    while !stop.load(Ordering::Relaxed) {
        // Answers from the window (accept / reject / forget a computer).
        while let Ok(cmd) = commands.try_recv() {
            match cmd {
                HostCommand::Accept { trust } => {
                    if let State::Pending { name, .. } = &state {
                        let name = name.clone();
                        if let Some(key) = net.remote_static() {
                            if trust {
                                trusted.insert(key);
                                emit(HostEvent::Trusted { key: key_hex(&key), name: name.clone() });
                            }
                            activate(&net, &streamer, &emit, name.clone(), &key);
                            state = State::Active;
                        }
                    }
                }
                HostCommand::Reject => {
                    if matches!(state, State::Pending { .. }) {
                        for _ in 0..3 {
                            net.send(&Msg::Status(2));
                        }
                        hang_up(&mut state, injector.as_mut(), "rejected");
                    }
                }
                HostCommand::Untrust { key } => {
                    if let Ok(k) = lindesk_proto::session::from_hex32(&key) {
                        trusted.remove(&k);
                        if net.remote_static() == Some(k) && !matches!(state, State::Idle) {
                            hang_up(&mut state, injector.as_mut(), "removed from trusted");
                        }
                    }
                }
            }
        }

        match net.receive(&mut buf) {
            Rx::Nothing => {}
            Rx::Established => {
                // A fresh handshake replaces whatever was running.
                streamer.stop();
                if let Some(i) = injector.as_mut() {
                    i.release_all();
                }
                state = State::Idle;
            }
            Rx::Msg(msg, from) => match (&mut state, msg) {
                (_, Msg::Ping(t)) => net.send(&Msg::Pong(t)),
                (_, Msg::Bye) => hang_up(&mut state, injector.as_mut(), "disconnected"),
                (State::Idle, Msg::Hello { version, name }) => {
                    let Some(key) = net.remote_static() else { continue };
                    if version != VERSION {
                        emit(HostEvent::Error { message: format!("The other computer runs an incompatible version ({version}); update both.") });
                        for _ in 0..3 {
                            net.send(&Msg::Status(2));
                        }
                        net.drop_session();
                        continue;
                    }
                    let name = if name.trim().is_empty() { from.ip().to_string() } else { name };
                    if opts.accept_all || trusted.contains(&key) {
                        activate(&net, &streamer, &emit, name.clone(), &key);
                        state = State::Active;
                    } else {
                        emit(HostEvent::ConnectRequest {
                            name: name.clone(),
                            addr: from.to_string(),
                            key: key_hex(&key),
                            fingerprint: fingerprint(&key),
                            verification: net.verification(),
                        });
                        net.send(&Msg::Status(0));
                        state = State::Pending { name, since: Instant::now(), last_ping: Instant::now() };
                    }
                }
                (State::Pending { .. }, Msg::Hello { .. }) => net.send(&Msg::Status(0)),
                (State::Active, Msg::Hello { .. }) => net.send(&Msg::Status(1)),
                (State::Active, Msg::RequestKeyframe) => streamer.force_keyframe(),
                (State::Active, Msg::Stats { frames_ok, frames_lost }) => {
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
                // Input only ever reaches the system from an approved client.
                (State::Active, Msg::Input { seq, ev }) => {
                    if let Some(inj) = injector.as_mut() {
                        inj.apply(seq, ev);
                    }
                }
                _ => {}
            },
        }

        match &mut state {
            State::Pending { since, last_ping, .. } => {
                if since.elapsed() > PENDING_TIMEOUT {
                    for _ in 0..3 {
                        net.send(&Msg::Status(2));
                    }
                    hang_up(&mut state, injector.as_mut(), "no answer");
                } else if last_ping.elapsed() >= Duration::from_secs(1) {
                    *last_ping = Instant::now();
                    net.send(&Msg::Status(0));
                }
            }
            State::Active => {
                if net.idle() > SESSION_TIMEOUT {
                    hang_up(&mut state, injector.as_mut(), "timed out");
                    kbps = opts.bitrate;
                } else if last_stats.elapsed() >= Duration::from_secs(1) {
                    last_stats = Instant::now();
                    emit(HostEvent::Stats {
                        fps: net.frames_sent.swap(0, Ordering::Relaxed),
                        kbps,
                        fec_permille: net.fec_permille.load(Ordering::Relaxed),
                    });
                }
            }
            State::Idle => {
                if net.peer().is_some() && net.idle() > SESSION_TIMEOUT {
                    net.drop_session(); // handshake finished but the client never said hello
                }
            }
        }
    }
    hang_up(&mut state, injector.as_mut(), "stopped");
    Ok(())
}

fn activate(net: &Net, streamer: &Streamer, emit: &impl Fn(HostEvent), name: String, key: &[u8; 32]) {
    net.send(&Msg::Status(1));
    if let Err(e) = streamer.start() {
        emit(HostEvent::Error { message: format!("cannot start streaming: {e:#}") });
    }
    streamer.force_keyframe();
    emit(HostEvent::ClientConnected {
        name,
        addr: net.peer().map(|a| a.to_string()).unwrap_or_default(),
        fingerprint: fingerprint(key),
        verification: net.verification(),
    });
}
