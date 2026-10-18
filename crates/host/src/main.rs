mod inject;
mod net;
mod pipeline;
mod portal;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use lindesk_proto::{Msg, VERSION, generate_key, parse_key};

use net::Net;
use pipeline::{Encoder, Settings, Source, Streamer};

#[derive(Parser)]
#[command(about = "lindesk host: shares this Wayland screen with one authenticated client")]
struct Args {
    /// UDP address to listen on.
    #[arg(long, default_value = "0.0.0.0:7000")]
    listen: SocketAddr,
    /// File holding the 64-hex-char pre-shared key (created on first run).
    #[arg(long)]
    key_file: Option<PathBuf>,
    /// portal | test | node:<pipewire-node-id>
    #[arg(long, default_value = "portal")]
    source: String,
    #[arg(long, value_enum, default_value = "auto")]
    encoder: Encoder,
    #[arg(long, default_value_t = 60)]
    fps: u32,
    /// Maximum (and initial) bitrate in kbit/s; lowered automatically on loss.
    #[arg(long, default_value_t = 20_000)]
    bitrate: u32,
    /// Do not draw the cursor into the video.
    #[arg(long)]
    no_cursor: bool,
    /// Do not accept keyboard/mouse input from the client.
    #[arg(long)]
    no_input: bool,
    /// Forget the remembered screen/window choice and exit.
    #[arg(long)]
    forget_screen: bool,
}

const SESSION_TIMEOUT: Duration = Duration::from_secs(5);
const MIN_KBPS: u32 = 2_000;

fn load_key(path: Option<PathBuf>) -> Result<[u8; 32]> {
    let path = match path {
        Some(p) => p,
        None => directories::ProjectDirs::from("", "", "lindesk")
            .context("no home directory")?
            .config_dir()
            .join("key"),
    };
    if !path.exists() {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&path, generate_key() + "\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        log::info!("created new key at {}", path.display());
    }
    let text = std::fs::read_to_string(&path)?;
    eprintln!("key file: {}  (give its content to the client with --key)", path.display());
    parse_key(&text)
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();
    if args.forget_screen {
        portal::forget_choice();
        return Ok(());
    }
    let key = load_key(args.key_file.clone())?;
    let net = Arc::new(Net::bind(args.listen, &key)?);

    // Keep the portal session alive for the whole run; dropping it ends the capture.
    let mut _portal = None;
    let source = match args.source.as_str() {
        "test" => Source::Test,
        "portal" => {
            let p = portal::open(!args.no_cursor)?;
            let fd = Arc::new(p.fd.try_clone()?);
            let node = p.node;
            _portal = Some(p);
            Source::Portal { fd, node }
        }
        s => Source::Node(s.strip_prefix("node:").and_then(|n| n.parse().ok()).context("--source: portal | test | node:<id>")?),
    };

    let streamer = Streamer::new(
        net.clone(),
        source,
        Settings { encoder: args.encoder, fps: args.fps, kbps: args.bitrate },
    )?;
    let mut injector = if args.no_input {
        None
    } else {
        match inject::Injector::new() {
            Ok(i) => Some(i),
            Err(e) => {
                log::warn!("input injection disabled: {e:#}");
                None
            }
        }
    };

    log::info!("listening on {}", args.listen);
    let mut buf = vec![0u8; lindesk_proto::MAX_DATAGRAM];
    let mut last_rx = Instant::now();
    let mut kbps = args.bitrate;
    let mut calm = 0u32;

    loop {
        match net.sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                let Ok(msg) = net.open(&buf[..n]) else { continue }; // forged/replayed: drop silently
                last_rx = Instant::now();
                // Authenticated packet: follow the peer even if its address changed.
                if net.peer() != Some(from) {
                    log::info!("client at {from}");
                    net.set_peer(Some(from));
                }
                match msg {
                    Msg::Hello { version } => {
                        if version != VERSION {
                            log::warn!("client protocol {version} != {VERSION}");
                        }
                        if let Err(e) = streamer.start() {
                            log::error!("cannot start streaming: {e:#}");
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
                                if kbps < args.bitrate {
                                    kbps = (kbps * 108 / 100).min(args.bitrate);
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
                    Msg::Bye => disconnect(&streamer, &net, injector.as_mut()),
                    Msg::Video(_) | Msg::Pong(_) => {}
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => log::warn!("recv: {e}"),
        }
        if net.peer().is_some() && last_rx.elapsed() > SESSION_TIMEOUT {
            log::info!("client timed out");
            disconnect(&streamer, &net, injector.as_mut());
            kbps = args.bitrate;
        }
    }
}

fn disconnect(streamer: &Streamer, net: &Net, inj: Option<&mut inject::Injector>) {
    streamer.stop();
    net.set_peer(None);
    if let Some(i) = inj {
        i.release_all();
    }
}
