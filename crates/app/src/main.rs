mod config;
mod deps;
mod gui;
mod i18n;
mod netinfo;
mod procs;
mod update;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lindesk_proto::events::{HostCommand, HostEvent, ViewerEvent, to_line};
use lindesk_proto::session::from_hex32;
use lindesk_proto::id;

#[derive(Parser)]
#[command(name = "lindesk", version, about = "Low-latency remote desktop with a Wayland host")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Share this screen (normally started by the app window).
    Host(HostArgs),
    /// Open a remote screen (normally started by the app window).
    View(ViewArgs),
}

#[derive(Parser)]
struct HostArgs {
    #[arg(long)]
    listen: Option<SocketAddr>,
    /// portal | test | node:<pipewire-node-id>
    #[arg(long, default_value = "portal")]
    source: String,
    #[arg(long, default_value = "auto")]
    encoder: String,
    #[arg(long, default_value_t = 60)]
    fps: u32,
    #[arg(long, default_value_t = 20_000)]
    bitrate: u32,
    #[arg(long)]
    no_cursor: bool,
    #[arg(long)]
    no_input: bool,
    /// Accept every connection without asking (tests only).
    #[arg(long)]
    accept_all: bool,
    /// Print status events as JSON lines, read commands as JSON lines, stop when stdin closes.
    #[arg(long)]
    json: bool,
    /// Forget the remembered screen/window choice and exit.
    #[arg(long)]
    forget_screen: bool,
}

#[derive(Parser)]
struct ViewArgs {
    /// The other computer's 10-digit ID (optionally `:port`).
    target: String,
    /// Hex public key the host must present (remembered from an earlier visit).
    #[arg(long)]
    pin: Option<String>,
    /// Name shown in the host's accept dialog.
    #[arg(long)]
    name: Option<String>,
    #[arg(long, default_value = "auto")]
    decoder: String,
    #[arg(long)]
    fullscreen: bool,
    #[arg(long)]
    json: bool,
    /// Decode without a window and exit after N seconds (tests).
    #[arg(long)]
    headless_seconds: Option<u64>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // Children are managed by the window and talk JSON on stdout; keep logs on stderr.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,iced_winit=warn,wgpu=warn,naga=warn,iced_wgpu=warn,winit=warn,zbus=warn,tracing=warn")).init();
    match cli.cmd {
        None => gui::run(),
        Some(Cmd::Host(a)) => run_host(a),
        Some(Cmd::View(a)) => run_view(a),
    }
}

fn run_host(a: HostArgs) -> Result<()> {
    if a.forget_screen {
        lindesk_host::portal::forget_choice();
        return Ok(());
    }
    let cfg = config::Config::load();
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel::<HostCommand>();
    {
        // The window owns this process through stdin: commands in, EOF means stop.
        // Without a window, a person at a terminal answers y / a / n.
        let (s, json) = (stop.clone(), a.json);
        std::thread::spawn(move || {
            for line in std::io::stdin().lines().map_while(|l| l.ok()) {
                let cmd = if json {
                    lindesk_proto::events::from_line::<HostCommand>(&line)
                } else {
                    match line.trim() {
                        "y" => Some(HostCommand::Accept { trust: false }),
                        "a" => Some(HostCommand::Accept { trust: true }),
                        "n" => Some(HostCommand::Reject),
                        _ => None,
                    }
                };
                if let Some(c) = cmd {
                    let _ = tx.send(c);
                }
            }
            s.store(true, Ordering::Relaxed);
        });
    }
    let json = a.json;
    let standalone = !a.json;
    let emit = move |ev: HostEvent| {
        if json {
            println!("{}", to_line(&ev));
            return;
        }
        match &ev {
            HostEvent::ConnectRequest { name, addr, verification, .. } => {
                eprintln!("{name} ({addr}) wants to connect, verification code {verification}. Accept? [y] once  [a] always  [n] no");
            }
            HostEvent::Trusted { key, name } if standalone => persist_trusted(key, name),
            other => eprintln!("{other:?}"),
        }
    };
    let trusted = cfg.trusted.iter().filter_map(|t| from_hex32(&t.key).ok()).collect();
    let result = lindesk_host::run(
        lindesk_host::Options {
            listen: a.listen.unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], cfg.port))),
            identity: config::identity(),
            trusted,
            accept_all: a.accept_all,
            source: a.source,
            encoder: parse_encoder(&a.encoder)?,
            fps: a.fps,
            bitrate: a.bitrate,
            cursor: !a.no_cursor,
            input: !a.no_input,
        },
        stop,
        rx,
        &emit,
    );
    if let Err(e) = &result {
        emit(HostEvent::Error { message: format!("{e:#}") });
    }
    result
}

/// Terminal mode has no window to remember trusted computers for us.
fn persist_trusted(key: &str, name: &str) {
    let mut cfg = config::Config::load();
    if !cfg.trusted.iter().any(|t| t.key == key) {
        cfg.trusted.push(config::Trusted { key: key.to_string(), name: name.to_string() });
        cfg.save();
    }
}

fn parse_encoder(s: &str) -> Result<lindesk_host::Encoder> {
    use lindesk_host::Encoder;
    Ok(match s {
        "auto" => Encoder::Auto,
        "va" => Encoder::Va,
        "nv" => Encoder::Nv,
        "x264" => Encoder::X264,
        o => anyhow::bail!("unknown encoder '{o}' (auto|va|nv|x264)"),
    })
}

fn parse_decoder(s: &str) -> Result<lindesk_viewer::Decoder> {
    use lindesk_viewer::Decoder;
    Ok(match s {
        "auto" => Decoder::Auto,
        "va" => Decoder::Va,
        "nv" => Decoder::Nv,
        "sw" => Decoder::Sw,
        o => anyhow::bail!("unknown decoder '{o}' (auto|va|nv|sw)"),
    })
}

fn run_view(a: ViewArgs) -> Result<()> {
    let host = id::parse(&a.target)?;
    let id_text = id::of(host).context("IPv6 hosts have no ID")?;
    let cfg = config::Config::load();
    let pinned = match &a.pin {
        Some(k) => Some(from_hex32(k).context("--pin must be 64 hex characters")?),
        None if !a.json => cfg.pinned_for(&id_text),
        None => None,
    };
    let json = a.json;
    let standalone = !a.json;
    let emit = move |ev: ViewerEvent| {
        if json {
            println!("{}", to_line(&ev));
            return;
        }
        match ev {
            ViewerEvent::Stats { fps, lost, rtt_ms } => eprintln!("fps={fps} lost={lost} rtt={rtt_ms:.1}ms"),
            ViewerEvent::HostIdentity { key, fingerprint } if standalone => {
                let mut cfg = config::Config::load();
                if cfg.pinned_for(&id_text).is_none() {
                    cfg.pin(&id_text, &key);
                    eprintln!("Remembered this computer's identity ({fingerprint}).");
                }
            }
            other => eprintln!("{other:?}"),
        }
    };
    let result = lindesk_viewer::run(
        lindesk_viewer::Options {
            host,
            identity: config::identity(),
            name: a.name.unwrap_or_else(config::computer_name),
            pinned,
            decoder: parse_decoder(&a.decoder)?,
            fullscreen: a.fullscreen,
            headless_seconds: a.headless_seconds,
        },
        emit,
    );
    if let Err(e) = &result {
        if json {
            println!("{}", to_line(&ViewerEvent::Error { message: format!("{e:#}") }));
        }
    }
    result
}
