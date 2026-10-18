//! The application window: share this PC, connect to another, settings, updates.

mod theme;
mod view;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use iced::{Subscription, Task};
use lindesk_proto::events::{HostEvent, ViewerEvent};
use lindesk_proto::{code, generate_key};

use crate::config::{Config, Quality};
use crate::netinfo::{self, Route};
use crate::procs::{self, Bus, Handle};
use crate::update::{self, Release};
use crate::deps;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Share,
    Connect,
    Settings,
}

pub enum HostState {
    Stopped,
    Starting,
    Waiting { encoder: String },
    Connected { addr: String, fps: u32, kbps: u32 },
}

pub enum ViewerState {
    Idle,
    Connecting(Instant),
    Connected { fps: u32, rtt_ms: f32 },
    Reconnecting,
}

pub enum UpdateState {
    Checking,
    UpToDate,
    Available(Release),
    Downloading { tag: String, progress: f32 },
    Failed(String),
    Disabled,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Info,
    Error,
}

pub struct App {
    page: Page,
    cfg: Config,
    missing: Vec<String>,
    rx: Receiver<Bus>,
    tx: Sender<Bus>,
    routes: Vec<Route>,
    route: Option<Route>,
    public_busy: bool,
    host: HostState,
    host_child: Option<Handle>,
    viewer: ViewerState,
    viewer_child: Option<Handle>,
    code_input: String,
    notice: Option<(Kind, String)>,
    update: UpdateState,
    copied_at: Option<Instant>,
    port_text: String,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Tick,
    Nav(Page),
    StartHost,
    StopHost,
    CopyCode,
    NewKey,
    PickRoute(Route),
    DetectPublic,
    CodeInput(String),
    Connect,
    ConnectRecent(usize),
    RemoveRecent(usize),
    Disconnect,
    SetQuality(Quality),
    SetEncoder(view::Opt),
    SetDecoder(view::Opt),
    PortInput(String),
    Cursor(bool),
    Control(bool),
    Fullscreen(bool),
    AutoUpdate(bool),
    CheckUpdate,
    InstallUpdate,
    ForgetScreen,
    DismissNotice,
}

pub fn run() -> anyhow::Result<()> {
    // Hybrid-GPU laptops: the integrated GPU is plenty for this window and does
    // not wake the discrete one that the game (or the encoder) may be using.
    if std::env::var_os("WGPU_POWER_PREF").is_none() {
        // SAFETY: called before any thread is started.
        unsafe { std::env::set_var("WGPU_POWER_PREF", "low") };
    }
    iced::application(App::new, App::update, App::view)
        .title(|_: &App| "lindesk".to_string())
        .font(include_bytes!("../../assets/Figtree.ttf").as_slice())
        .default_font(theme::FONT)
        .subscription(App::subscription)
        .theme(|_: &App| theme::app_theme())
        .window(iced::window::Settings {
            size: iced::Size::new(1000.0, 660.0),
            min_size: Some(iced::Size::new(820.0, 560.0)),
            icon: iced::window::icon::from_file_data(include_bytes!("../../assets/icon.png"), None).ok(),
            platform_specific: iced::window::settings::PlatformSpecific { application_id: "lindesk".into(), ..Default::default() },
            ..Default::default()
        })
        .run()?;
    Ok(())
}

impl App {
    fn new() -> (Self, Task<Msg>) {
        let cfg = Config::load();
        let (tx, rx) = channel();
        let routes = netinfo::local_routes();
        let route = routes.first().cloned();
        let disabled = std::env::var_os("LINDESK_NO_UPDATE").is_some();
        let update = if disabled { UpdateState::Disabled } else { UpdateState::Checking };
        if !disabled {
            spawn_check(tx.clone());
        }
        let port_text = cfg.port.to_string();
        (
            Self {
                page: Page::Share,
                missing: deps::check().missing,
                cfg,
                rx,
                tx,
                routes,
                route,
                public_busy: false,
                host: HostState::Stopped,
                host_child: None,
                viewer: ViewerState::Idle,
                viewer_child: None,
                code_input: String::new(),
                notice: None,
                update,
                copied_at: None,
                port_text,
            },
            Task::none(),
        )
    }

    fn subscription(&self) -> Subscription<Msg> {
        iced::time::every(Duration::from_millis(250)).map(|_| Msg::Tick)
    }

    fn busy(&self) -> bool {
        !matches!(self.host, HostState::Stopped) || !matches!(self.viewer, ViewerState::Idle)
    }

    fn share_code(&self) -> Option<String> {
        let route = self.route.as_ref()?;
        Some(code::encode(SocketAddr::new(route.ip.into(), self.cfg.port), &self.cfg.key()))
    }

    fn fail(&mut self, msg: impl Into<String>) {
        self.notice = Some((Kind::Error, msg.into()));
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Tick => self.drain(),
            Msg::Nav(p) => {
                self.page = p;
                self.notice = None;
            }
            Msg::StartHost => {
                if self.route.is_none() {
                    self.fail("No network address found. Connect to a network first.");
                } else {
                    match procs::spawn_host(&self.cfg, self.cfg.port, self.tx.clone()) {
                        Ok(h) => {
                            self.host_child = Some(h);
                            self.host = HostState::Starting;
                            self.notice = None;
                        }
                        Err(e) => self.fail(format!("Could not start sharing: {e:#}")),
                    }
                }
            }
            Msg::StopHost => {
                if let Some(h) = self.host_child.take() {
                    h.stop();
                }
                self.host = HostState::Stopped;
            }
            Msg::CopyCode => {
                if let Some(c) = self.share_code() {
                    self.copied_at = Some(Instant::now());
                    return iced::clipboard::write(c);
                }
            }
            Msg::NewKey => {
                if matches!(self.host, HostState::Stopped) {
                    self.cfg.host_key = generate_key();
                    self.cfg.save();
                    self.notice = Some((Kind::Info, "New code created. The old one no longer works.".into()));
                } else {
                    self.fail("Stop sharing first, then create a new code.");
                }
            }
            Msg::PickRoute(r) => self.route = Some(r),
            Msg::DetectPublic => {
                self.public_busy = true;
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(Bus::PublicIp(netinfo::public_ip()));
                });
            }
            Msg::CodeInput(s) => self.code_input = s,
            Msg::Connect => {
                let c = self.code_input.trim().to_string();
                self.connect(&c);
            }
            Msg::ConnectRecent(i) => {
                if let Some(r) = self.cfg.recent.get(i).cloned() {
                    self.connect(&r.code);
                }
            }
            Msg::RemoveRecent(i) => {
                if i < self.cfg.recent.len() {
                    self.cfg.recent.remove(i);
                    self.cfg.save();
                }
            }
            Msg::Disconnect => {
                if let Some(h) = self.viewer_child.take() {
                    h.stop();
                }
                self.viewer = ViewerState::Idle;
            }
            Msg::SetQuality(q) => {
                self.cfg.quality = q;
                self.cfg.save();
            }
            Msg::SetEncoder(o) => {
                self.cfg.encoder = o.value.into();
                self.cfg.save();
            }
            Msg::SetDecoder(o) => {
                self.cfg.decoder = o.value.into();
                self.cfg.save();
            }
            Msg::PortInput(s) => {
                self.port_text = s.chars().filter(char::is_ascii_digit).take(5).collect();
                if let Ok(p) = self.port_text.parse::<u16>() {
                    if p >= 1024 && p != self.cfg.port {
                        self.cfg.port = p;
                        self.cfg.save();
                    }
                }
            }
            Msg::Cursor(v) => {
                self.cfg.cursor = v;
                self.cfg.save();
            }
            Msg::Control(v) => {
                self.cfg.allow_control = v;
                self.cfg.save();
            }
            Msg::Fullscreen(v) => {
                self.cfg.fullscreen = v;
                self.cfg.save();
            }
            Msg::AutoUpdate(v) => {
                self.cfg.auto_update = v;
                self.cfg.save();
            }
            Msg::CheckUpdate => {
                self.update = UpdateState::Checking;
                spawn_check(self.tx.clone());
            }
            Msg::InstallUpdate => self.install_update(),
            Msg::ForgetScreen => {
                lindesk_host::portal::forget_choice();
                self.notice = Some((Kind::Info, "Done. Next time you share, the system asks which screen to use.".into()));
            }
            Msg::DismissNotice => self.notice = None,
        }
        Task::none()
    }

    fn connect(&mut self, code_text: &str) {
        if !matches!(self.viewer, ViewerState::Idle) {
            return;
        }
        match code::decode(code_text) {
            Err(e) => self.fail(format!("{e}")),
            Ok((addr, _)) => match procs::spawn_viewer(&self.cfg, code_text, self.tx.clone()) {
                Ok(h) => {
                    self.viewer_child = Some(h);
                    self.viewer = ViewerState::Connecting(Instant::now());
                    self.cfg.remember(code_text, addr.ip().to_string());
                    self.notice = None;
                }
                Err(e) => self.fail(format!("Could not open the viewer: {e:#}")),
            },
        }
    }

    fn install_update(&mut self) {
        let UpdateState::Available(rel) = &self.update else { return };
        if self.busy() {
            self.fail("Stop sharing and disconnect before updating.");
            return;
        }
        let rel = rel.clone();
        self.update = UpdateState::Downloading { tag: rel.tag.clone(), progress: 0.0 };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let t2 = tx.clone();
            match update::install(&rel, &move |p| {
                let _ = t2.send(Bus::UpdateProgress(p));
            }) {
                Ok(()) => {
                    let _ = tx.send(Bus::UpdateInstalled);
                }
                Err(e) => {
                    let _ = tx.send(Bus::UpdateFailed(format!("{e:#}")));
                }
            }
        });
    }

    fn drain(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Bus::Host(ev) => match ev {
                    HostEvent::Listening { encoder, .. } => self.host = HostState::Waiting { encoder },
                    HostEvent::ClientConnected { addr } => self.host = HostState::Connected { addr, fps: 0, kbps: 0 },
                    HostEvent::ClientLeft { .. } => {
                        let enc = self.encoder_name();
                        self.host = HostState::Waiting { encoder: enc };
                    }
                    HostEvent::Stats { fps, kbps, .. } => {
                        if let HostState::Connected { fps: f, kbps: k, .. } = &mut self.host {
                            (*f, *k) = (fps, kbps);
                        }
                    }
                    HostEvent::Error { message } => self.fail(message),
                },
                Bus::HostExit(how) => {
                    self.host_child = None;
                    if !matches!(self.host, HostState::Stopped) && !how.is_empty() {
                        self.fail(format!("Sharing stopped unexpectedly ({how}). Details: ~/.config/lindesk/logs/host.log"));
                    }
                    self.host = HostState::Stopped;
                }
                Bus::Viewer(ev) => match ev {
                    ViewerEvent::Connecting => {}
                    ViewerEvent::Connected => self.viewer = ViewerState::Connected { fps: 0, rtt_ms: 0.0 },
                    ViewerEvent::Stats { fps, rtt_ms, .. } => self.viewer = ViewerState::Connected { fps, rtt_ms },
                    ViewerEvent::Reconnecting => self.viewer = ViewerState::Reconnecting,
                    ViewerEvent::Closed => {}
                    ViewerEvent::Error { message } => self.fail(message),
                },
                Bus::ViewerExit(how) => {
                    self.viewer_child = None;
                    if matches!(self.viewer, ViewerState::Connecting(_)) && self.notice.is_none() {
                        self.fail(if how.is_empty() { "The viewer closed before connecting.".to_string() } else { format!("The viewer failed ({how}). Details: ~/.config/lindesk/logs/view.log") });
                    }
                    self.viewer = ViewerState::Idle;
                }
                Bus::PublicIp(ip) => {
                    self.public_busy = false;
                    match ip {
                        Some(ip) => self.add_public(ip),
                        None => self.fail("Could not detect the public address. Check your internet connection."),
                    }
                }
                Bus::UpdateFound(r) => match r {
                    Ok(None) => self.update = UpdateState::UpToDate,
                    Ok(Some(rel)) => {
                        self.update = UpdateState::Available(rel);
                        if self.cfg.auto_update {
                            self.install_update();
                        }
                    }
                    Err(e) => {
                        log::warn!("update check failed: {e}");
                        self.update = UpdateState::Failed(e);
                    }
                },
                Bus::UpdateProgress(p) => {
                    if let UpdateState::Downloading { progress, .. } = &mut self.update {
                        *progress = p;
                    }
                }
                Bus::UpdateInstalled => update::restart(),
                Bus::UpdateFailed(e) => {
                    log::warn!("update failed: {e}");
                    self.update = UpdateState::Failed(e);
                }
            }
        }
        if let ViewerState::Connecting(t) = self.viewer {
            if t.elapsed() > CONNECT_TIMEOUT {
                if let Some(h) = self.viewer_child.take() {
                    h.stop();
                }
                self.viewer = ViewerState::Idle;
                self.fail(format!(
                    "No answer from the other computer. Check that it is sharing, that the code is current, and that UDP port {} is reachable (firewall / router).",
                    code::decode(&self.cfg.recent.first().map(|r| r.code.clone()).unwrap_or_default()).map(|(a, _)| a.port()).unwrap_or(self.cfg.port)
                ));
            }
        }
        if self.copied_at.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            self.copied_at = None;
        }
    }

    fn encoder_name(&self) -> String {
        match &self.host {
            HostState::Waiting { encoder } => encoder.clone(),
            _ => "auto".into(),
        }
    }

    fn add_public(&mut self, ip: Ipv4Addr) {
        let r = Route { label: "Internet (needs port forwarding)".into(), ip };
        self.routes.retain(|x| !x.label.starts_with("Internet"));
        self.routes.push(r.clone());
        self.route = Some(r);
    }
}

fn spawn_check(tx: Sender<Bus>) {
    std::thread::spawn(move || {
        let r = update::check(VERSION).map_err(|e| format!("{e:#}"));
        let _ = tx.send(Bus::UpdateFound(r));
    });
}
