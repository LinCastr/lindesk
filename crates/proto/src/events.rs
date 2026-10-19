//! Status events that the host and viewer processes print as JSON lines, so
//! the GUI can follow them without sharing a process (a crash in capture or
//! decode cannot take the interface down).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum HostEvent {
    Listening { addr: String, encoder: String },
    /// Someone unknown asks to connect; answer with [`HostCommand::Accept`] or [`HostCommand::Reject`].
    ConnectRequest { name: String, addr: String, key: String, fingerprint: String, verification: String },
    ClientConnected { name: String, addr: String, fingerprint: String, verification: String },
    /// The owner chose "always allow this computer": remember this public key.
    Trusted { key: String, name: String },
    ClientLeft { reason: String },
    /// Once per second while a client is connected.
    Stats { fps: u32, kbps: u32, fec_permille: u32 },
    Error { message: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ViewerEvent {
    Connecting,
    /// Reached the host; waiting for its owner to accept.
    Waiting { verification: String },
    /// The host's identity, for first-use pinning.
    HostIdentity { key: String, fingerprint: String },
    Connected,
    /// The owner said no.
    Denied,
    Stats { fps: u32, lost: u32, rtt_ms: f32 },
    Reconnecting,
    Closed,
    Error { message: String },
}

/// Commands the window sends to the host process, one JSON line each on stdin.
/// End of input means "stop".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum HostCommand {
    Accept { trust: bool },
    Reject,
    /// Forget a trusted computer (hex public key); disconnects it if connected now.
    Untrust { key: String },
}

pub fn to_line<T: Serialize>(ev: &T) -> String {
    serde_json::to_string(ev).unwrap_or_default()
}

pub fn from_line<'a, T: Deserialize<'a>>(line: &'a str) -> Option<T> {
    serde_json::from_str(line).ok()
}
