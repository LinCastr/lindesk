//! The application window: share this PC, connect to another, settings, updates.

mod theme;
mod view;

use std::net::Ipv4Addr;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use iced::{Subscription, Task};
use lindesk_proto::events::{HostCommand, HostEvent, ViewerEvent};
use lindesk_proto::id;

use crate::config::{Config, Lang, Trusted};
use crate::i18n::Tx;
use crate::netinfo::{self, Kind, Route};
use crate::procs::{self, Bus, Handle};
use crate::update::{self, Release};
use crate::deps;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

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
    Connected { name: String, fps: u32, kbps: u32 },
}

pub enum ViewerState {
    Idle,
    Connecting,
    Waiting { verification: String },
    Connected { fps: u32, rtt_ms: f32 },
}

/// Someone unknown asking to connect to this computer.
#[derive(Clone)]
pub struct Request {
    pub name: String,
    pub verification: String,
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
pub enum Kind2 {
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
    show_routes: bool,
    public_busy: bool,
    host: HostState,
    host_child: Option<Handle>,
    request: Option<Request>,
    viewer: ViewerState,
    viewer_child: Option<Handle>,
    viewer_id: Option<String>,
    id_input: String,
    notice: Option<(Kind2, String)>,
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
    CopyId,
    PickRoute(Route),
    ToggleRoutes,
    DetectPublic,
    Accept(bool),
    Reject,
    IdInput(String),
    Connect,
    ConnectRecent(usize),
    RemoveRecent(usize),
    Disconnect,
    SetLang(Lang),
    SetQuality(view::QOpt),
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
    RemoveTrusted(usize),
    DismissNotice,
}

/// The installer puts the menu icon in the user's icon folder; keep it in step with
/// the icon built into this version (updates replace only the binary).
fn refresh_menu_icon() {
    let Some(home) = std::env::var_os("HOME") else { return };
    let path = std::path::Path::new(&home).join(".local/share/icons/hicolor/256x256/apps/lindesk.png");
    let embedded = include_bytes!("../../assets/icon.png");
    if path.exists() && std::fs::read(&path).is_ok_and(|b| b != embedded) {
        let _ = std::fs::write(&path, embedded);
    }
}

pub fn run() -> anyhow::Result<()> {
    refresh_menu_icon();
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
            size: iced::Size::new(1000.0, 680.0),
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
        update::installed_exe(); // remember where we live before any update swaps the file
        let cfg = Config::load();
        cfg.save();
        let (tx, rx) = channel();
        let disabled = std::env::var_os("LINDESK_NO_UPDATE").is_some();
        let update = if disabled { UpdateState::Disabled } else { UpdateState::Checking };
        if !disabled {
            spawn_check(tx.clone());
        }
        let port_text = cfg.port.to_string();
        (
            Self {
                page: match std::env::var("LINDESK_START_PAGE").as_deref() {
                    Ok("connect") => Page::Connect,
                    Ok("settings") => Page::Settings,
                    _ => Page::Share,
                },
                missing: deps::check().missing,
                cfg,
                rx,
                tx,
                routes: netinfo::local_routes(),
                show_routes: false,
                public_busy: false,
                host: HostState::Stopped,
                host_child: None,
                request: None,
                viewer: ViewerState::Idle,
                viewer_child: None,
                viewer_id: None,
                id_input: String::new(),
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

    fn tx(&self) -> Tx {
        Tx(self.cfg.lang)
    }

    fn busy(&self) -> bool {
        !matches!(self.host, HostState::Stopped) || !matches!(self.viewer, ViewerState::Idle)
    }

    /// The route the ID is built from.
    fn route(&self) -> Option<&Route> {
        netinfo::pick(&self.routes, self.cfg.route_pref.as_deref())
    }

    fn my_id(&self) -> Option<String> {
        self.route().map(|r| id::encode(r.ip))
    }

    fn fail(&mut self, msg: impl Into<String>) {
        self.notice = Some((Kind2::Error, msg.into()));
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Tick => self.drain(),
            Msg::Nav(p) => {
                self.page = p;
                self.notice = None;
            }
            Msg::StartHost => {
                if self.route().is_none() {
                    let m = self.tx().no_network();
                    self.fail(m);
                } else {
                    match procs::spawn_host(&self.cfg, self.cfg.port, self.tx.clone()) {
                        Ok(h) => {
                            self.host_child = Some(h);
                            self.host = HostState::Starting;
                            self.notice = None;
                        }
                        Err(e) => {
                            let m = self.tx().start_failed(&format!("{e:#}"));
                            self.fail(m);
                        }
                    }
                }
            }
            Msg::StopHost => {
                if let Some(h) = self.host_child.take() {
                    h.stop();
                }
                self.host = HostState::Stopped;
                self.request = None;
            }
            Msg::CopyId => {
                if let Some(i) = self.my_id() {
                    self.copied_at = Some(Instant::now());
                    return iced::clipboard::write(id::pretty(&i));
                }
            }
            Msg::PickRoute(r) => {
                self.cfg.route_pref = Some(r.iface);
                self.cfg.save();
            }
            Msg::ToggleRoutes => self.show_routes = !self.show_routes,
            Msg::DetectPublic => {
                self.public_busy = true;
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(Bus::PublicIp(netinfo::public_ip()));
                });
            }
            Msg::Accept(trust) => {
                if self.request.take().is_some() {
                    if let Some(h) = self.host_child.as_mut() {
                        h.command(&HostCommand::Accept { trust });
                    }
                }
            }
            Msg::Reject => {
                if self.request.take().is_some() {
                    if let Some(h) = self.host_child.as_mut() {
                        h.command(&HostCommand::Reject);
                    }
                }
            }
            Msg::IdInput(s) => {
                let cleaned: String = s.chars().filter(|c| c.is_ascii_digit() || *c == ':').collect();
                self.id_input = if cleaned.len() == 10 && !cleaned.contains(':') { id::pretty(&cleaned) } else { cleaned };
            }
            Msg::Connect => {
                let t = self.id_input.clone();
                self.connect(&t);
            }
            Msg::ConnectRecent(i) => {
                if let Some(r) = self.cfg.recent.get(i).cloned() {
                    self.connect(&r.id);
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
            Msg::SetLang(l) => {
                self.cfg.lang = l;
                self.cfg.save();
            }
            Msg::SetQuality(o) => {
                self.cfg.quality = o.0;
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
                self.notice = Some((Kind2::Info, self.tx().forgot_done().into()));
            }
            Msg::RemoveTrusted(i) => {
                if i < self.cfg.trusted.len() {
                    let t = self.cfg.trusted.remove(i);
                    self.cfg.save();
                    if let Some(h) = self.host_child.as_mut() {
                        h.command(&HostCommand::Untrust { key: t.key });
                    }
                }
            }
            Msg::DismissNotice => self.notice = None,
        }
        Task::none()
    }

    fn connect(&mut self, text: &str) {
        if !matches!(self.viewer, ViewerState::Idle) {
            return;
        }
        let Ok(addr) = id::parse(text) else {
            let m = self.tx().bad_id();
            self.fail(m);
            return;
        };
        let Some(id_text) = id::of(addr) else { return };
        let target = if addr.port() == id::DEFAULT_PORT { id_text.clone() } else { format!("{id_text}:{}", addr.port()) };
        let pin = self.cfg.pinned_for(&id_text).map(|k| lindesk_proto::session::key_hex(&k));
        match procs::spawn_viewer(&self.cfg, &target, pin, self.tx.clone()) {
            Ok(h) => {
                self.viewer_child = Some(h);
                self.viewer = ViewerState::Connecting;
                self.viewer_id = Some(id_text);
                self.notice = None;
            }
            Err(e) => {
                let m = self.tx().viewer_failed(&format!("{e:#}"));
                self.fail(m);
            }
        }
    }

    fn install_update(&mut self) {
        let UpdateState::Available(rel) = &self.update else { return };
        if self.busy() {
            let m = self.tx().upd_busy();
            self.fail(m);
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
                    HostEvent::ConnectRequest { name, verification, .. } => {
                        self.request = Some(Request { name, verification });
                    }
                    HostEvent::ClientConnected { name, .. } => {
                        self.request = None;
                        self.host = HostState::Connected { name, fps: 0, kbps: 0 };
                    }
                    HostEvent::ClientLeft { .. } => {
                        self.request = None;
                        if !matches!(self.host, HostState::Stopped) {
                            self.host = HostState::Waiting { encoder: "auto".into() };
                        }
                    }
                    HostEvent::Trusted { key, name } => {
                        if !self.cfg.trusted.iter().any(|t| t.key == key) {
                            self.cfg.trusted.push(Trusted { key, name });
                            self.cfg.save();
                        }
                    }
                    HostEvent::Stats { fps, kbps, .. } => {
                        if let HostState::Connected { fps: f, kbps: k, .. } = &mut self.host {
                            (*f, *k) = (fps, kbps);
                        }
                    }
                    HostEvent::Error { message } => {
                        let m = self.tx().localize(&message);
                        self.fail(m);
                    }
                },
                Bus::HostExit(how) => {
                    self.host_child = None;
                    self.request = None;
                    if !matches!(self.host, HostState::Stopped) && !how.is_empty() {
                        let m = self.tx().host_crashed(&how);
                        self.fail(m);
                    }
                    self.host = HostState::Stopped;
                }
                Bus::Viewer(ev) => match ev {
                    ViewerEvent::Connecting | ViewerEvent::Closed | ViewerEvent::Denied => {}
                    ViewerEvent::Waiting { verification } => self.viewer = ViewerState::Waiting { verification },
                    ViewerEvent::HostIdentity { key, .. } => {
                        // First visit: remember who this ID really is, so an impostor is caught next time.
                        if let Some(id_text) = self.viewer_id.clone() {
                            if self.cfg.pinned_for(&id_text).is_none() {
                                self.cfg.pin(&id_text, &key);
                            }
                        }
                    }
                    ViewerEvent::Connected => {
                        self.viewer = ViewerState::Connected { fps: 0, rtt_ms: 0.0 };
                        if let Some(id_text) = self.viewer_id.clone() {
                            self.cfg.remember(&id_text, id::pretty(&id_text));
                        }
                    }
                    ViewerEvent::Stats { fps, rtt_ms, .. } => self.viewer = ViewerState::Connected { fps, rtt_ms },
                    ViewerEvent::Reconnecting => {}
                    ViewerEvent::Error { message } => {
                        let m = self.tx().localize(&message);
                        self.fail(m);
                    }
                },
                Bus::ViewerExit(how) => {
                    self.viewer_child = None;
                    if !matches!(self.viewer, ViewerState::Connected { .. }) && self.notice.is_none() {
                        let m = if how.is_empty() { self.tx().viewer_closed().to_string() } else { self.tx().viewer_failed(&how) };
                        self.fail(m);
                    }
                    self.viewer = ViewerState::Idle;
                }
                Bus::PublicIp(ip) => {
                    self.public_busy = false;
                    match ip {
                        Some(ip) => self.add_public(ip),
                        None => {
                            let m = self.tx().public_failed();
                            self.fail(m);
                        }
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
        if self.copied_at.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            self.copied_at = None;
        }
    }

    fn add_public(&mut self, ip: Ipv4Addr) {
        let r = Route { kind: Kind::Internet, iface: "internet".into(), ip };
        self.routes.retain(|x| x.kind != Kind::Internet);
        self.routes.push(r);
        self.cfg.route_pref = Some("internet".into());
        self.cfg.save();
    }
}

fn spawn_check(tx: Sender<Bus>) {
    std::thread::spawn(move || {
        let r = update::check(VERSION).map_err(|e| format!("{e:#}"));
        let _ = tx.send(Bus::UpdateFound(r));
    });
}
