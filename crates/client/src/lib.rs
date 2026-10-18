mod decode;
mod session;
mod window;

pub use decode::Decoder;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Result;
use lindesk_proto::events::ViewerEvent;

use decode::{Decode, Latest, Picture};
use session::Session;

pub struct Options {
    pub host: SocketAddr,
    pub key: [u8; 32],
    pub decoder: Decoder,
    pub fullscreen: bool,
    /// Decode without a window for `seconds`, then exit (tests).
    pub headless_seconds: Option<u64>,
}

/// Opens the viewer and blocks until the window closes.
pub fn run(opts: Options, emit: impl Fn(ViewerEvent) + Send + Sync + 'static) -> Result<()> {
    let session = Session::connect(opts.host, &opts.key)?;
    let latest: Latest = Arc::new(Mutex::new(Picture::default()));
    let emit: Arc<dyn Fn(ViewerEvent) + Send + Sync> = Arc::new(emit);
    emit(ViewerEvent::Connecting);

    if let Some(secs) = opts.headless_seconds {
        let frames = Arc::new(AtomicU64::new(0));
        let f = frames.clone();
        let decode = Arc::new(Decode::new(opts.decoder, latest, move || {
            f.fetch_add(1, Ordering::Relaxed);
        })?);
        session.run(decode, emit.clone());
        std::thread::sleep(Duration::from_secs(secs));
        session.bye();
        anyhow::ensure!(frames.load(Ordering::Relaxed) > 0, "no frames decoded");
        return Ok(());
    }

    let s2 = session.clone();
    let l2 = latest.clone();
    let emit2 = emit.clone();
    let result = window::run(session, latest, opts.fullscreen, move |proxy| {
        let p2 = proxy.clone();
        let decode = Decode::new(opts.decoder, l2, move || {
            let _ = proxy.send_event(window::UserEvent::Frame);
        })
        .expect("decoder");
        // Stats also go to the window title, the only HUD for now.
        let wrapped: Arc<dyn Fn(ViewerEvent) + Send + Sync> = Arc::new(move |ev| {
            if let ViewerEvent::Stats { fps, rtt_ms, .. } = &ev {
                let _ = p2.send_event(window::UserEvent::Title(format!("lindesk — {fps} fps · {rtt_ms:.0} ms")));
            }
            emit2(ev);
        });
        s2.run(Arc::new(decode), wrapped);
    });
    emit(ViewerEvent::Closed);
    result
}
