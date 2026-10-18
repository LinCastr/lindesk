//! Runs the host and viewer as child processes of the window, so a crash in
//! capture, encode or decode cannot take the interface down, and closing the
//! window always ends the session.

use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::{Context, Result};
use lindesk_proto::events::{HostEvent, ViewerEvent, from_line};

use crate::config::{self, Config};
use crate::update::Release;

pub enum Bus {
    Host(HostEvent),
    HostExit(String),
    Viewer(ViewerEvent),
    ViewerExit(String),
    UpdateFound(Result<Option<Release>, String>),
    UpdateProgress(f32),
    UpdateInstalled,
    UpdateFailed(String),
    PublicIp(Option<std::net::Ipv4Addr>),
}

/// A running child. Dropping the handle closes its stdin, which asks it to stop.
pub struct Handle {
    pid: u32,
    stdin: Option<ChildStdin>,
}

impl Handle {
    /// Ask politely (stdin EOF); if it is still alive after a grace period, kill it.
    pub fn stop(mut self) {
        drop(self.stdin.take());
        let pid = self.pid;
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(4));
            // Exited children are gone from /proc; only a straggler gets killed.
            if std::path::Path::new(&format!("/proc/{pid}")).exists() {
                let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).status();
            }
        });
    }
}

fn spawn<E: serde::de::DeserializeOwned + Send + 'static>(
    sub: &str,
    args: Vec<String>,
    env: Vec<(&str, String)>,
    tx: Sender<Bus>,
    on_event: fn(E) -> Bus,
    on_exit: fn(String) -> Bus,
) -> Result<Handle> {
    let exe = std::env::current_exe().context("own executable")?;
    let log_dir = config::dir().join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let log = std::fs::File::create(log_dir.join(format!("{sub}.log"))).map(Stdio::from).unwrap_or_else(|_| Stdio::null());
    let mut cmd = Command::new(exe);
    cmd.arg(sub).args(args).arg("--json").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(log);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child: Child = cmd.spawn().with_context(|| format!("start {sub}"))?;
    let stdin = child.stdin.take();
    let out = child.stdout.take().context("child stdout")?;
    let pid = child.id();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(|l| l.ok()) {
            if let Some(ev) = from_line::<E>(&line) {
                if tx.send(on_event(ev)).is_err() {
                    return;
                }
            }
        }
        let how = match child.wait() {
            Ok(s) if s.success() => String::new(),
            Ok(s) => format!("exited with {s}"),
            Err(e) => e.to_string(),
        };
        let _ = tx.send(on_exit(how));
    });
    Ok(Handle { pid, stdin })
}

pub fn spawn_host(cfg: &Config, listen_port: u16, tx: Sender<Bus>) -> Result<Handle> {
    let (fps, kbps) = cfg.quality.params();
    let mut args = vec![
        "--listen".into(),
        format!("0.0.0.0:{listen_port}"),
        "--encoder".into(),
        cfg.encoder.clone(),
        "--fps".into(),
        fps.to_string(),
        "--bitrate".into(),
        kbps.to_string(),
    ];
    if !cfg.cursor {
        args.push("--no-cursor".into());
    }
    if !cfg.allow_control {
        args.push("--no-input".into());
    }
    spawn("host", args, vec![("LINDESK_KEY", cfg.host_key.clone())], tx, Bus::Host, Bus::HostExit)
}

pub fn spawn_viewer(cfg: &Config, code: &str, tx: Sender<Bus>) -> Result<Handle> {
    let mut args = vec!["--decoder".into(), cfg.decoder.clone()];
    if cfg.fullscreen {
        args.push("--fullscreen".into());
    }
    spawn("view", args, vec![("LINDESK_CODE", code.to_string())], tx, Bus::Viewer, Bus::ViewerExit)
}
