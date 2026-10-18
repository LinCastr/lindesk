mod config;
mod deps;
mod gui;
mod netinfo;
mod procs;
mod update;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lindesk_proto::events::{HostEvent, ViewerEvent, to_line};
use lindesk_proto::{code, parse_key};

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
    /// Hex key. Prefer the LINDESK_KEY environment variable: arguments show up in `ps`.
    #[arg(long, env = "LINDESK_KEY", hide_env_values = true)]
    key: Option<String>,
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
    /// Print status events as JSON lines and stop when stdin closes.
    #[arg(long)]
    json: bool,
    /// Forget the remembered screen/window choice and exit.
    #[arg(long)]
    forget_screen: bool,
}

#[derive(Parser)]
struct ViewArgs {
    /// Connection code (LD-...) or host:port together with --key.
    #[arg(env = "LINDESK_CODE", hide_env_values = true)]
    target: String,
    #[arg(long, env = "LINDESK_KEY", hide_env_values = true)]
    key: Option<String>,
    #[arg(long)]
    key_file: Option<PathBuf>,
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
    let key = match a.key {
        Some(k) => parse_key(&k)?,
        None => cfg.key(),
    };
    let stop = Arc::new(AtomicBool::new(false));
    if a.json {
        // The window owns this process: when it closes our stdin (or dies), stop.
        let s = stop.clone();
        std::thread::spawn(move || {
            let mut sink = Vec::new();
            let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut sink);
            s.store(true, Ordering::Relaxed);
        });
    }
    let json = a.json;
    let emit = move |ev: HostEvent| {
        if json {
            println!("{}", to_line(&ev));
        } else {
            eprintln!("{ev:?}");
        }
    };
    let result = lindesk_host::run(
        lindesk_host::Options {
            listen: a.listen.unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], cfg.port))),
            key,
            source: a.source,
            encoder: parse_encoder(&a.encoder)?,
            fps: a.fps,
            bitrate: a.bitrate,
            cursor: !a.no_cursor,
            input: !a.no_input,
        },
        stop,
        &emit,
    );
    if let Err(e) = &result {
        emit(HostEvent::Error { message: format!("{e:#}") });
    }
    result
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
    let (host, key) = if a.target.trim_start().get(..2).is_some_and(|p| p.eq_ignore_ascii_case("LD")) {
        code::decode(&a.target)?
    } else {
        let key_text = match (&a.key, &a.key_file) {
            (Some(k), _) => k.clone(),
            (None, Some(f)) => std::fs::read_to_string(f).with_context(|| format!("read {}", f.display()))?,
            (None, None) => anyhow::bail!("give a connection code, or host:port with --key/--key-file"),
        };
        let addr = std::net::ToSocketAddrs::to_socket_addrs(&a.target.as_str())
            .context("resolve host")?
            .next()
            .context("host resolved to nothing")?;
        (addr, parse_key(&key_text)?)
    };
    let json = a.json;
    let emit = move |ev: ViewerEvent| {
        if json {
            println!("{}", to_line(&ev));
        } else if let ViewerEvent::Stats { fps, lost, rtt_ms } = ev {
            eprintln!("fps={fps} lost={lost} rtt={rtt_ms:.1}ms");
        }
    };
    let result = lindesk_viewer::run(
        lindesk_viewer::Options {
            host,
            key,
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
