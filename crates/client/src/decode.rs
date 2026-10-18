//! H.264 access units in, BGRx pixels out. Prefers hardware decoders.

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Decoder {
    Auto,
    Va,
    Nv,
    Sw,
}

/// Latest decoded picture. The window thread always takes the newest one;
/// older pictures are overwritten, never queued.
#[derive(Default)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    /// Tightly packed little-endian `0x00RRGGBB` pixels (BGRx bytes).
    pub pixels: Vec<u32>,
    pub serial: u64,
}

pub type Latest = Arc<Mutex<Picture>>;

fn has(f: &str) -> bool {
    gst::ElementFactory::find(f).is_some()
}

fn pick(want: Decoder) -> Result<&'static str> {
    let order: &[(Decoder, &str)] = &[(Decoder::Va, "vah264dec"), (Decoder::Nv, "nvh264dec"), (Decoder::Sw, "avdec_h264")];
    for (d, f) in order {
        if (want == Decoder::Auto || want == *d) && has(f) {
            return Ok(f);
        }
    }
    bail!("no usable H.264 decoder ({want:?})")
}

pub struct Decode {
    pipeline: gst::Pipeline,
    src: gst_app::AppSrc,
}

impl Decode {
    /// `on_frame` fires (from a streaming thread) after each new picture is stored.
    pub fn new(want: Decoder, latest: Latest, on_frame: impl Fn() + Send + Sync + 'static) -> Result<Self> {
        gst::init().context("gstreamer init")?;
        let dec = pick(want)?;
        log::info!("decoder: {dec}");
        let desc = format!(
            "appsrc name=src is-live=true format=time do-timestamp=true max-bytes=2000000 \
             leaky-type=upstream caps=video/x-h264,stream-format=byte-stream,alignment=au ! \
             h264parse ! {dec} ! videoconvert ! video/x-raw,format=BGRx ! \
             appsink name=out sync=false max-buffers=1 drop=true emit-signals=false"
        );
        let pipeline = gst::parse::launch(&desc)?
            .downcast::<gst::Pipeline>()
            .map_err(|_| anyhow::anyhow!("not a pipeline"))?;
        let src = pipeline.by_name("src").context("appsrc")?.downcast::<gst_app::AppSrc>().map_err(|_| anyhow::anyhow!("appsrc"))?;
        let sink = pipeline.by_name("out").context("appsink")?.downcast::<gst_app::AppSink>().map_err(|_| anyhow::anyhow!("appsink"))?;
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |s| {
                    let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let caps = sample.caps().ok_or(gst::FlowError::Error)?;
                    let info = gst_video::VideoInfo::from_caps(caps).map_err(|_| gst::FlowError::Error)?;
                    let buf = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let frame = gst_video::VideoFrameRef::from_buffer_ref_readable(buf, &info).map_err(|_| gst::FlowError::Error)?;
                    let (w, h) = (info.width() as usize, info.height() as usize);
                    let stride = info.stride()[0] as usize;
                    let data = frame.plane_data(0).map_err(|_| gst::FlowError::Error)?;
                    {
                        let mut pic = latest.lock().unwrap();
                        pic.width = w;
                        pic.height = h;
                        pic.pixels.resize(w * h, 0);
                        for y in 0..h {
                            let row = &data[y * stride..y * stride + w * 4];
                            for (dst, px) in pic.pixels[y * w..(y + 1) * w].iter_mut().zip(row.chunks_exact(4)) {
                                *dst = u32::from_le_bytes([px[0], px[1], px[2], 0]);
                            }
                        }
                        pic.serial += 1;
                    }
                    on_frame();
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
        pipeline.set_state(gst::State::Playing).context("start decoder")?;
        Ok(Self { pipeline, src })
    }

    pub fn push(&self, data: Vec<u8>) {
        let _ = self.src.push_buffer(gst::Buffer::from_slice(data));
    }
}

impl Drop for Decode {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
