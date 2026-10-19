//! Settings and paired computers, stored as JSON in `~/.config/lindesk/config.json`.

use std::path::PathBuf;

use lindesk_proto::Identity;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    Pt,
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Pt, Lang::En];

    /// Portuguese for pt_* locales, English otherwise.
    pub fn detect() -> Self {
        let loc = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).unwrap_or_default();
        if loc.to_lowercase().starts_with("pt") { Lang::Pt } else { Lang::En }
    }
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Lang::Pt => "Português (Brasil)",
            Lang::En => "English",
        })
    }
}

/// A computer we connected to before.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recent {
    pub name: String,
    /// 10-digit ID (digits only).
    pub id: String,
}

/// A computer allowed to connect without asking.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trusted {
    /// Hex public key.
    pub key: String,
    pub name: String,
}

/// First-use pinned identity of a host we connected to.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KnownHost {
    pub id: String,
    pub key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub lang: Lang,
    pub port: u16,
    /// Network interface the ID is built from (None = best available).
    pub route_pref: Option<String>,
    pub trusted: Vec<Trusted>,
    pub known_hosts: Vec<KnownHost>,
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
            lang: Lang::detect(),
            port: 7000,
            route_pref: None,
            trusted: Vec::new(),
            known_hosts: Vec::new(),
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
    if let Some(d) = std::env::var_os("LINDESK_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    directories::ProjectDirs::from("", "", "lindesk")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn path() -> PathBuf {
    dir().join("config.json")
}

impl Config {
    /// Reads the file without writing it: only the window saves, so the host and
    /// viewer processes can never overwrite what the window changed.
    pub fn load() -> Self {
        std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
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

    pub fn remember(&mut self, id: &str, name: String) {
        self.recent.retain(|r| r.id != id);
        self.recent.insert(0, Recent { name, id: id.to_string() });
        self.recent.truncate(8);
        self.save();
    }

    pub fn pinned_for(&self, id: &str) -> Option<[u8; 32]> {
        self.known_hosts.iter().find(|k| k.id == id).and_then(|k| lindesk_proto::session::from_hex32(&k.key).ok())
    }

    pub fn pin(&mut self, id: &str, key: &str) {
        self.known_hosts.retain(|k| k.id != id);
        self.known_hosts.push(KnownHost { id: id.to_string(), key: key.to_string() });
        self.save();
    }
}

/// This computer's long-term identity, created on first use (owner-only file).
pub fn identity() -> Identity {
    let path = dir().join("identity.key");
    if let Some(id) = std::fs::read_to_string(&path).ok().and_then(|t| Identity::from_hex(&t).ok()) {
        return id;
    }
    let id = Identity::generate();
    let _ = std::fs::create_dir_all(dir());
    if std::fs::write(&path, id.to_hex()).is_ok() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    id
}

/// Name shown to the other side's owner.
pub fn computer_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "Linux".to_string())
}
