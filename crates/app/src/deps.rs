//! Checks the system pieces lindesk relies on and says what to install.

use gstreamer as gst;

pub struct Report {
    pub missing: Vec<String>,
}

fn has(f: &str) -> bool {
    gst::ElementFactory::find(f).is_some()
}

pub fn check() -> Report {
    let mut missing = Vec::new();
    if gst::init().is_err() {
        missing.push("GStreamer (gst-plugins-base)".to_string());
        return Report { missing };
    }
    let any = |l: &[&str]| l.iter().any(|f| has(f));
    if !has("pipewiresrc") {
        missing.push("PipeWire GStreamer plugin (gst-plugin-pipewire)".into());
    }
    if !has("h264parse") || !has("appsink") {
        missing.push("GStreamer base/bad plugins (gst-plugins-base, gst-plugins-bad)".into());
    }
    if !any(&["vah264enc", "nvh264enc", "x264enc"]) {
        missing.push("an H.264 encoder (gst-plugins-bad for VA-API/NVENC, or gst-plugins-ugly for x264)".into());
    }
    if !any(&["vah264dec", "nvh264dec", "avdec_h264"]) {
        missing.push("an H.264 decoder (gst-plugins-bad for VA-API/NVDEC, or gst-libav)".into());
    }
    Report { missing }
}
