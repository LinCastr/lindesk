//! GStreamer capture → H.264 encode → UDP fragments.
//!
//! The pipeline only runs while a client is connected, so an idle host costs
//! nothing (important when the machine is also used for gaming).

use std::os::fd::{IntoRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use lindesk_proto::fragment;

use crate::net::Net;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Encoder {
    Auto,
    Va,
    Nv,
    X264,
}

pub enum Source {
    /// Screen via the xdg-desktop-portal PipeWire remote.
    Portal { fd: Arc<OwnedFd>, node: u32 },
    /// An existing PipeWire video node (no portal prompt).
    Node(u32),
    /// Synthetic moving picture, for tests that must not touch the screen.
    Test,
}

pub struct Settings {
    pub encoder: Encoder,
    pub fps: u32,
    pub kbps: u32,
}

fn has(factory: &str) -> bool {
    gst::ElementFactory::find(factory).is_some()
}

pub fn pick_encoder(want: Encoder) -> Result<Encoder> {
    let order = match want {
        Encoder::Auto => vec![Encoder::Va, Encoder::Nv, Encoder::X264],
        e => vec![e],
    };
    for e in order {
        let ok = match e {
            Encoder::Va => has("vah264enc") && has("vapostproc"),
            Encoder::Nv => has("nvh264enc"),
            Encoder::X264 => has("x264enc"),
            Encoder::Auto => false,
        };
        if ok {
            return Ok(e);
        }
    }
    bail!("no usable H.264 encoder ({want:?}); install gst-plugins-bad (va/nvcodec) or gst-plugins-ugly (x264)")
}

fn source_desc(src: &Source, fps: u32) -> Result<String> {
    Ok(match src {
        // pipewiresrc takes ownership of the fd, so hand it a duplicate.
        Source::Portal { fd, node } => format!(
            "pipewiresrc fd={} path={node} do-timestamp=true keepalive-time=250 ! \
             queue leaky=downstream max-size-buffers=2 ! videorate drop-only=true max-rate={fps}",
            fd.try_clone()?.into_raw_fd()
        ),
        Source::Node(node) => format!(
            "pipewiresrc path={node} do-timestamp=true keepalive-time=250 ! \
             queue leaky=downstream max-size-buffers=2 ! videorate drop-only=true max-rate={fps}"
        ),
        Source::Test => format!(
            "videotestsrc is-live=true pattern=ball ! \
             video/x-raw,width=1920,height=1080,framerate={fps}/1"
        ),
    })
}

/// Encoder segment. Low-latency settings: no B-frames, one reference frame,
/// CBR so a keyframe cannot blow the bitrate budget by much, and a long GOP
/// because keyframes are requested on demand after packet loss.
fn encoder_desc(enc: Encoder, fps: u32, kbps: u32) -> String {
    let gop = fps * 4;
    match enc {
        Encoder::Va => format!(
            "vapostproc ! video/x-raw(memory:VAMemory),format=NV12 ! \
             vah264enc name=enc rate-control=cbr bitrate={kbps} target-usage=7 \
             b-frames=0 ref-frames=1 key-int-max={gop} aud=false"
        ),
        Encoder::Nv => format!(
            "videoconvert ! video/x-raw,format=NV12 ! \
             nvh264enc name=enc rc-mode=cbr bitrate={kbps} preset=low-latency-hq \
             zerolatency=true bframes=0 gop-size={gop} repeat-sequence-header=true"
        ),
        Encoder::X264 => format!(
            "videoconvert ! video/x-raw,format=I420 ! \
             x264enc name=enc tune=zerolatency speed-preset=ultrafast bitrate={kbps} \
             key-int-max={gop} bframes=0 sliced-threads=true"
        ),
        Encoder::Auto => unreachable!(),
    }
}

pub struct Streamer {
    net: Arc<Net>,
    src: Source,
    set: Settings,
    enc_kind: Encoder,
    running: Mutex<Option<Running>>,
    frame_no: Arc<AtomicU32>,
}

struct Running {
    pipeline: gst::Pipeline,
    enc: gst::Element,
    alive: Arc<AtomicBool>,
}

impl Streamer {
    pub fn new(net: Arc<Net>, src: Source, set: Settings) -> Result<Self> {
        gst::init().context("gstreamer init")?;
        let enc_kind = pick_encoder(set.encoder)?;
        log::info!("encoder: {enc_kind:?}");
        Ok(Self { net, src, set, enc_kind, running: Mutex::new(None), frame_no: Arc::new(AtomicU32::new(0)) })
    }


    pub fn start(&self) -> Result<()> {
        let mut slot = self.running.lock().unwrap();
        if slot.is_some() {
            return Ok(());
        }
        let desc = format!(
            "{} ! {} ! h264parse config-interval=-1 ! \
             video/x-h264,stream-format=byte-stream,alignment=au ! \
             appsink name=out sync=false max-buffers=2 drop=true emit-signals=false",
            source_desc(&self.src, self.set.fps)?,
            encoder_desc(self.enc_kind, self.set.fps, self.set.kbps),
        );
        log::debug!("pipeline: {desc}");
        let pipeline = gst::parse::launch(&desc)
            .context("build pipeline")?
            .downcast::<gst::Pipeline>()
            .map_err(|_| anyhow::anyhow!("not a pipeline"))?;
        let enc = pipeline.by_name("enc").context("encoder element")?;
        let sink = pipeline
            .by_name("out")
            .context("appsink")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| anyhow::anyhow!("appsink type"))?;

        let net = self.net.clone();
        let frame_no = self.frame_no.clone();
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |s| {
                    let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let buf = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let map = buf.map_readable().map_err(|_| gst::FlowError::Error)?;
                    let key = !buf.flags().contains(gst::BufferFlags::DELTA_UNIT);
                    let n = frame_no.fetch_add(1, Ordering::Relaxed);
                    let fec = net.fec_permille.load(Ordering::Relaxed);
                    net.send_all(&fragment(n, key, map.as_slice(), fec));
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        // Surface pipeline errors in the log instead of dying silently.
        let alive = Arc::new(AtomicBool::new(true));
        let bus = pipeline.bus().context("bus")?;
        let flag = alive.clone();
        std::thread::Builder::new().name("gst-bus".into()).spawn(move || {
            while flag.load(Ordering::Relaxed) {
                if let Some(m) = bus.timed_pop_filtered(
                    gst::ClockTime::from_mseconds(500),
                    &[gst::MessageType::Error, gst::MessageType::Warning],
                ) {
                    match m.view() {
                        gst::MessageView::Error(e) => {
                            log::error!("gst error: {} ({:?})", e.error(), e.debug())
                        }
                        gst::MessageView::Warning(w) => {
                            log::warn!("gst warning: {} ({:?})", w.error(), w.debug())
                        }
                        _ => {}
                    }
                }
            }
        })?;

        pipeline.set_state(gst::State::Playing).context("start pipeline")?;
        log::info!("streaming started");
        *slot = Some(Running { pipeline, enc, alive });
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(r) = self.running.lock().unwrap().take() {
            r.alive.store(false, Ordering::Relaxed);
            let _ = r.pipeline.set_state(gst::State::Null);
            log::info!("streaming stopped");
        }
    }

    /// Next frame is an IDR with fresh SPS/PPS.
    pub fn force_keyframe(&self) {
        if let Some(r) = self.running.lock().unwrap().as_ref() {
            let ev = gstreamer_video::UpstreamForceKeyUnitEvent::builder()
                .all_headers(true)
                .build();
            r.enc.send_event(ev);
        }
    }

    pub fn set_bitrate(&self, kbps: u32) {
        if let Some(r) = self.running.lock().unwrap().as_ref() {
            r.enc.set_property("bitrate", kbps);
        }
    }
}
