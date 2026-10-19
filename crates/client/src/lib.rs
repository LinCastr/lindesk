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
    pub identity: lindesk_proto::Identity,
    /// Display name of this computer, shown in the host's accept dialog.
    pub name: String,
    /// Host key remembered from an earlier visit; a different key aborts.
    pub pinned: Option<[u8; 32]>,
    pub decoder: Decoder,
    pub fullscreen: bool,
    /// Decode without a window for `seconds`, then exit (tests).
    pub headless_seconds: Option<u64>,
}

/// Connects, waits for the host's owner to accept, then opens the viewer and
/// blocks until the window closes.
pub fn run(opts: Options, emit: impl Fn(ViewerEvent) + Send + Sync + 'static) -> Result<()> {
    let emit: Arc<dyn Fn(ViewerEvent) + Send + Sync> = Arc::new(emit);
    emit(ViewerEvent::Connecting);
    let session = Session::connect(opts.host, &opts.identity, opts.name.clone(), opts.pinned, &*emit)?;
    session.wait_accept(&*emit)?;
    let latest: Latest = Arc::new(Mutex::new(Picture::default()));

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
    let verification = session.verification();
    let result = window::run(session, latest, opts.fullscreen, move |proxy| {
        let p2 = proxy.clone();
        let decode = Decode::new(opts.decoder, l2, move || {
            let _ = proxy.send_event(window::UserEvent::Frame);
        })
        .expect("decoder");
        // Stats also go to the window title, the only HUD for now.
        let wrapped: Arc<dyn Fn(ViewerEvent) + Send + Sync> = Arc::new(move |ev| {
            match &ev {
                ViewerEvent::Stats { fps, rtt_ms, .. } => {
                    let _ = p2.send_event(window::UserEvent::Title(format!("lindesk — {fps} fps · {rtt_ms:.0} ms · {verification}")));
                }
                ViewerEvent::Error { .. } => {
                    let _ = p2.send_event(window::UserEvent::Quit);
                }
                _ => {}
            }
            emit2(ev);
        });
        s2.run(Arc::new(decode), wrapped);
    });
    emit(ViewerEvent::Closed);
    result
}
