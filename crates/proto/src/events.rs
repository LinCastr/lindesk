//! Status events that the host and viewer processes print as JSON lines, so
//! the GUI can follow them without sharing a process (a crash in capture or
//! decode cannot take the interface down).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum HostEvent {
    Listening { addr: String, encoder: String },
    ClientConnected { addr: String },
    ClientLeft { reason: String },
    /// Once per second while a client is connected.
    Stats { fps: u32, kbps: u32, fec_permille: u32 },
    Error { message: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ViewerEvent {
    Connecting,
    Connected,
    Stats { fps: u32, lost: u32, rtt_ms: f32 },
    Reconnecting,
    Closed,
    Error { message: String },
}

pub fn to_line<T: Serialize>(ev: &T) -> String {
    serde_json::to_string(ev).unwrap_or_default()
}

pub fn from_line<'a, T: Deserialize<'a>>(line: &'a str) -> Option<T> {
    serde_json::from_str(line).ok()
}
