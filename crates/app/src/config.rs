//! Settings and paired computers, stored as JSON in `~/.config/lindesk/config.json`.

use std::path::PathBuf;

use lindesk_proto::{generate_key, parse_key};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Balanced,
    HighQuality,
    LowBandwidth,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::Balanced, Quality::HighQuality, Quality::LowBandwidth];

    /// (fps, max kbit/s)
    pub fn params(self) -> (u32, u32) {
        match self {
            Quality::Balanced => (60, 20_000),
            Quality::HighQuality => (60, 40_000),
            Quality::LowBandwidth => (30, 6_000),
        }
    }
}

impl std::fmt::Display for Quality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Quality::Balanced => "Balanced · 60 fps, up to 20 Mbps",
            Quality::HighQuality => "High quality · 60 fps, up to 40 Mbps",
            Quality::LowBandwidth => "Low bandwidth · 30 fps, up to 6 Mbps",
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recent {
    pub name: String,
    pub code: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Secret shared with computers allowed to connect (hex).
    pub host_key: String,
    pub port: u16,
    pub quality: Quality,
    pub encoder: String,
    pub decoder: String,
    pub cursor: bool,
    pub allow_control: bool,
    pub fullscreen: bool,
    pub auto_update: bool,
    pub recent: Vec<Recent>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host_key: generate_key(),
            port: 7000,
            quality: Quality::Balanced,
            encoder: "auto".into(),
            decoder: "auto".into(),
            cursor: true,
            allow_control: true,
            fullscreen: false,
            auto_update: true,
            recent: Vec::new(),
        }
    }
}

pub fn dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "lindesk")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn path() -> PathBuf {
    dir().join("config.json")
}

impl Config {
    pub fn load() -> Self {
        let mut c: Config = std::fs::read_to_string(path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        if parse_key(&c.host_key).is_err() {
            c.host_key = generate_key();
        }
        c.save();
        c
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(dir());
        let tmp = path().with_extension("tmp");
        if let Ok(json) = serde_json::to_string_pretty(self) {
            if std::fs::write(&tmp, json).is_ok() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    // the file holds the secret key
                    let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
                }
                let _ = std::fs::rename(&tmp, path());
            }
        }
    }

    pub fn key(&self) -> [u8; 32] {
        parse_key(&self.host_key).expect("validated on load")
    }

    pub fn remember(&mut self, code: &str, name: String) {
        self.recent.retain(|r| r.code != code);
        self.recent.insert(0, Recent { name, code: code.to_string() });
        self.recent.truncate(8);
        self.save();
    }
}
