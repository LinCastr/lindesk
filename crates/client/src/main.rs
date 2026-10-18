mod decode;
mod session;
mod window;

use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use lindesk_proto::parse_key;

use decode::{Decode, Decoder, Latest, Picture};
use session::Session;

#[derive(Parser)]
#[command(about = "lindesk client: view and control a lindesk host")]
struct Args {
    /// Host address, e.g. 192.168.0.10:7000
    host: String,
    /// Pre-shared key (64 hex chars). Prefer --key-file: arguments show up in `ps`.
    #[arg(long, conflicts_with = "key_file")]
    key: Option<String>,
    #[arg(long)]
    key_file: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "auto")]
    decoder: Decoder,
    #[arg(long)]
    fullscreen: bool,
    /// Decode without a window and exit after --seconds (for tests).
    #[arg(long)]
    headless: bool,
    #[arg(long, default_value_t = 10)]
    seconds: u64,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();
    let host: SocketAddr = args
        .host
        .to_socket_addrs()
        .context("resolve host")?
        .next()
        .context("host resolved to nothing")?;
    let key_text = match (&args.key, &args.key_file) {
        (Some(k), _) => k.clone(),
        (None, Some(f)) => std::fs::read_to_string(f).with_context(|| format!("read {}", f.display()))?,
        (None, None) => anyhow::bail!("give --key or --key-file"),
    };
    let key = parse_key(&key_text)?;
    let session = Session::connect(host, &key)?;
    let latest: Latest = Arc::new(Mutex::new(Picture::default()));

    if args.headless {
        let frames = Arc::new(AtomicU64::new(0));
        let f = frames.clone();
        let decode = Arc::new(Decode::new(args.decoder, latest, move || {
            f.fetch_add(1, Ordering::Relaxed);
        })?);
        session.run(decode);
        std::thread::sleep(Duration::from_secs(args.seconds));
        session.bye();
        let n = frames.load(Ordering::Relaxed);
        println!("decoded {n} frames in {}s", args.seconds);
        anyhow::ensure!(n > 0, "no frames decoded");
        return Ok(());
    }

    let s2 = session.clone();
    let l2 = latest.clone();
    window::run(session, latest, args.fullscreen, move |proxy| {
        let decode = Decode::new(args.decoder, l2, move || {
            let _ = proxy.send_event(window::UserEvent::Frame);
        })
        .expect("decoder");
        s2.run(Arc::new(decode));
    })
}
