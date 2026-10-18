//! Layout of every screen.

use iced::widget::{
    Column, Space, button, column, container, pick_list, progress_bar, row, scrollable, stack, text, text_input, toggler,
};
use iced::widget::text::Wrapping;
use iced::{Alignment, Color, Element, Length};

use super::theme as t;
use super::{App, HostState, Kind, Msg, Page, UpdateState, VERSION, ViewerState};
use crate::config::Quality;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opt {
    pub value: &'static str,
    pub label: &'static str,
}

impl std::fmt::Display for Opt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label)
    }
}

const ENCODERS: [Opt; 4] = [
    Opt { value: "auto", label: "Automatic (recommended)" },
    Opt { value: "va", label: "Intel / AMD graphics (VA-API)" },
    Opt { value: "nv", label: "NVIDIA graphics (NVENC)" },
    Opt { value: "x264", label: "Processor (slower, works everywhere)" },
];

const DECODERS: [Opt; 4] = [
    Opt { value: "auto", label: "Automatic (recommended)" },
    Opt { value: "va", label: "Intel / AMD graphics (VA-API)" },
    Opt { value: "nv", label: "NVIDIA graphics (NVDEC)" },
    Opt { value: "sw", label: "Processor (slower, works everywhere)" },
];

fn pick<'a>(list: &'a [Opt], current: &str) -> Option<Opt> {
    list.iter().find(|o| o.value == current).cloned()
}

// --- small building blocks ----------------------------------------------------

fn heading<'a>(title: &'a str, sub: &'a str) -> Column<'a, Msg> {
    column![text(title).size(28).font(t::BOLD), text(sub).size(14).color(t::MUTED)].spacing(6)
}

fn card<'a>(body: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    container(body).padding(24).width(Length::Fill).style(t::card).into()
}

fn card_title<'a>(s: &'a str) -> Element<'a, Msg> {
    text(s).size(16).font(t::SEMIBOLD).into()
}

fn hint<'a>(s: impl text::IntoFragment<'a>) -> Element<'a, Msg> {
    text(s).size(13).color(t::MUTED).into()
}

fn dot<'a>(c: Color) -> Element<'a, Msg> {
    container(Space::new()).width(10).height(10).style(t::dot(c)).into()
}

fn btn<'a>(label: &'a str, msg: Option<Msg>, style: fn(&iced::Theme, button::Status) -> button::Style) -> Element<'a, Msg> {
    let b = button(container(text(label).size(14).font(t::SEMIBOLD)).padding([2, 6])).padding([8, 16]).style(style);
    match msg {
        Some(m) => b.on_press(m).into(),
        None => b.into(),
    }
}

fn setting_row<'a>(title: &'a str, sub: &'a str, control: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    row![
        column![text(title).size(14).font(t::SEMIBOLD), text(sub).size(13).color(t::MUTED)].spacing(2).width(Length::Fill),
        control.into()
    ]
    .spacing(16)
    .align_y(Alignment::Center)
    .into()
}

fn switch<'a>(on: bool, f: fn(bool) -> Msg) -> Element<'a, Msg> {
    toggler(on).on_toggle(f).into()
}

fn banner<'a>(kind: Kind, msg: &'a str) -> Element<'a, Msg> {
    let c = if kind == Kind::Error { t::BAD } else { t::GOOD };
    container(
        row![text(msg).size(14).width(Length::Fill), button(text("Dismiss").size(13)).style(t::link).on_press(Msg::DismissNotice)]
            .spacing(12)
            .align_y(Alignment::Center),
    )
    .padding([12, 16])
    .width(Length::Fill)
    .style(t::tint(c))
    .into()
}

// --- screens ------------------------------------------------------------------

impl App {
    pub fn view(&self) -> Element<'_, Msg> {
        let body = match self.page {
            Page::Share => self.share_page(),
            Page::Connect => self.connect_page(),
            Page::Settings => self.settings_page(),
        };
        let mut content = Column::new().spacing(20).max_width(760);
        if let Some((kind, msg)) = &self.notice {
            content = content.push(banner(*kind, msg));
        }
        content = content.push(body);
        let main = container(scrollable(container(content).padding(32).center_x(Length::Fill)).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(t::page);
        let base: Element<'_, Msg> = row![self.sidebar(), main].into();
        if let UpdateState::Downloading { tag, progress } = &self.update {
            let panel = card(
                column![
                    text(format!("Updating to {tag}")).size(20).font(t::BOLD),
                    progress_bar(0.0..=1.0, *progress).girth(8),
                    hint("lindesk restarts by itself when this finishes."),
                ]
                .spacing(16)
                .width(340),
            );
            let overlay = container(panel).center(Length::Fill).style(t::scrim);
            return stack![base, overlay].into();
        }
        base
    }

    fn sidebar(&self) -> Element<'_, Msg> {
        let item = |label: &'static str, page: Page| -> Element<'_, Msg> {
            button(text(label).size(14).font(t::SEMIBOLD).width(Length::Fill))
                .padding([10, 14])
                .width(Length::Fill)
                .style(t::nav(self.page == page))
                .on_press(Msg::Nav(page))
                .into()
        };
        let status = match &self.update {
            UpdateState::Checking => hint("Checking for updates…"),
            UpdateState::UpToDate => hint("You're up to date"),
            UpdateState::Disabled => hint("Updates disabled"),
            UpdateState::Failed(_) => hint("Couldn't check for updates"),
            UpdateState::Downloading { .. } => hint("Updating…"),
            UpdateState::Available(r) => column![
                text(format!("{} is available", r.tag)).size(13).color(t::GOOD),
                button(text("Update now").size(13)).style(t::link).padding(0).on_press(Msg::InstallUpdate)
            ]
            .spacing(2)
            .into(),
        };
        let nav = column![item("Share this PC", Page::Share), item("Connect", Page::Connect), item("Settings", Page::Settings)].spacing(4);
        let brand = row![
            iced::widget::image(iced::widget::image::Handle::from_bytes(include_bytes!("../../assets/icon.png").as_slice())).width(34).height(34),
            column![text("lindesk").size(20).font(t::BOLD), text("Remote desktop").size(12).color(t::MUTED)].spacing(0)
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        container(
            column![brand, Space::new().height(12), nav, Space::new().height(Length::Fill), hint(format!("Version {VERSION}")), status].spacing(8),
        )
        .padding(20)
        .width(230)
        .height(Length::Fill)
        .style(t::sidebar)
        .into()
    }

    fn share_page(&self) -> Element<'_, Msg> {
        let (dot_color, title, sub) = match &self.host {
            HostState::Stopped => (t::MUTED, "Not sharing".to_string(), "Start sharing to let someone connect.".to_string()),
            HostState::Starting => (t::WARN, "Starting…".to_string(), "Preparing the screen capture.".to_string()),
            HostState::Waiting { encoder } => (t::WARN, "Waiting for a connection".to_string(), format!("Ready. Encoding with {encoder}.")),
            HostState::Connected { addr, fps, kbps } => (
                t::GOOD,
                format!("Connected to {}", addr.rsplit_once(':').map_or(addr.as_str(), |(h, _)| h)),
                format!("{fps} fps · {:.1} Mbps", *kbps as f32 / 1000.0),
            ),
        };
        let running = !matches!(self.host, HostState::Stopped);
        let action = if running {
            btn("Stop sharing", Some(Msg::StopHost), t::danger)
        } else {
            btn("Start sharing", Some(Msg::StartHost), t::primary)
        };
        let status = card(
            row![
                dot(dot_color),
                column![text(title).size(18).font(t::SEMIBOLD), text(sub).size(14).color(t::MUTED)].spacing(2).width(Length::Fill),
                action
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        );

        let code_text = self.share_code().unwrap_or_else(|| "No network address found".into());
        let copy_label = if self.copied_at.is_some() { "Copied" } else { "Copy code" };
        let route_pick = pick_list(self.routes.as_slice(), self.route.clone(), Msg::PickRoute)
            .placeholder("No network found")
            .width(Length::Fill)
            .padding([9, 12])
            .style(t::select)
            .menu_style(t::select_menu);
        let detect = if self.public_busy {
            btn("Detecting…", None, t::secondary)
        } else {
            btn("Detect public address", Some(Msg::DetectPublic), t::secondary)
        };
        let code_card = card(
            column![
                card_title("Connection code"),
                hint("Send this code to the person who will connect. It contains the address and the secret key."),
                container(text(code_text).size(15).font(t::MONO).wrapping(Wrapping::Glyph)).padding(16).width(Length::Fill).style(t::code_box),
                row![btn(copy_label, Some(Msg::CopyCode), t::primary), btn("Create new code", Some(Msg::NewKey), t::secondary)].spacing(8),
                container(text("Anyone who has this code can see and control this computer. Share it only with people you trust.").size(13))
                    .padding([10, 14])
                    .width(Length::Fill)
                    .style(t::tint(t::WARN)),
            ]
            .spacing(14),
        );
        let route_card = card(
            column![
                card_title("How will they reach you?"),
                hint("Same Wi-Fi/network or a VPN: pick that address. Over the internet: detect your public address and forward UDP port to this computer on your router."),
                row![route_pick, detect].spacing(8),
                hint(format!("UDP port {} · change it in Settings", self.cfg.port)),
            ]
            .spacing(12),
        );
        let mut page = Column::new().spacing(20).push(heading("Share this computer", "Let someone see and control this screen."));
        if !self.missing.is_empty() {
            page = page.push(self.missing_banner());
        }
        page = page.push(status);
        if !running {
            page = page.push(hint("The first time, your system asks which screen or window to share. It remembers your choice."));
        }
        page.push(code_card).push(route_card).into()
    }

    fn missing_banner(&self) -> Element<'_, Msg> {
        let mut c = Column::new().spacing(4).push(text("Some required components are missing").size(14).font(t::SEMIBOLD));
        for m in &self.missing {
            c = c.push(text(format!("• {m}")).size(13));
        }
        c = c.push(text("Install them with your package manager, then reopen lindesk.").size(13));
        container(c).padding([12, 16]).width(Length::Fill).style(t::tint(t::WARN)).into()
    }

    fn connect_page(&self) -> Element<'_, Msg> {
        let idle = matches!(self.viewer, ViewerState::Idle);
        let input = text_input("Paste the connection code (LD-…)", &self.code_input)
            .on_input(Msg::CodeInput)
            .on_submit(Msg::Connect)
            .padding([11, 14])
            .size(15)
            .font(t::MONO)
            .style(t::field);
        let go = btn("Connect", (idle && !self.code_input.trim().is_empty()).then_some(Msg::Connect), t::primary);
        let entry = card(
            column![
                card_title("Connect to a computer"),
                hint("Ask the other person for their connection code and paste it here."),
                row![input, go].spacing(8).align_y(Alignment::Center),
            ]
            .spacing(12),
        );

        let session: Option<Element<'_, Msg>> = match &self.viewer {
            ViewerState::Idle => None,
            ViewerState::Connecting(_) => Some(self.session_card(t::WARN, "Connecting…".into(), "Waiting for the other computer to answer.".into())),
            ViewerState::Reconnecting => Some(self.session_card(t::WARN, "Connection lost".into(), "Trying to reconnect…".into())),
            ViewerState::Connected { fps, rtt_ms } => {
                Some(self.session_card(t::GOOD, "Connected".into(), format!("{fps} fps · {rtt_ms:.0} ms round trip. Press F11 in the viewer for full screen.")))
            }
        };

        let mut recents = Column::new().spacing(8).push(card_title("Recent computers"));
        if self.cfg.recent.is_empty() {
            recents = recents.push(hint("Computers you connect to will show up here for one-click access."));
        }
        for (i, r) in self.cfg.recent.iter().enumerate() {
            let line = row![
                column![text(&r.name).size(14).font(t::SEMIBOLD), text(short_code(&r.code)).size(12).font(t::MONO).color(t::MUTED)].spacing(2).width(Length::Fill),
                btn("Connect", idle.then_some(Msg::ConnectRecent(i)), t::secondary),
                button(text("Remove").size(13)).style(t::link).on_press(Msg::RemoveRecent(i)),
            ]
            .spacing(10)
            .align_y(Alignment::Center);
            recents = recents.push(container(line).padding([10, 14]).width(Length::Fill).style(t::code_box));
        }

        let mut page = Column::new().spacing(20).push(heading("Connect", "Control another computer running lindesk."));
        if !self.missing.is_empty() {
            page = page.push(self.missing_banner());
        }
        page = page.push(entry);
        if let Some(s) = session {
            page = page.push(s);
        }
        page.push(card(recents)).into()
    }

    fn session_card(&self, c: Color, title: String, sub: String) -> Element<'_, Msg> {
        card(
            row![
                dot(c),
                column![text(title).size(18).font(t::SEMIBOLD), text(sub).size(14).color(t::MUTED)].spacing(2).width(Length::Fill),
                btn("Disconnect", Some(Msg::Disconnect), t::danger)
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        )
    }

    fn settings_page(&self) -> Element<'_, Msg> {
        let quality = pick_list(Quality::ALL.as_slice(), Some(self.cfg.quality), Msg::SetQuality)
            .width(300)
            .padding([9, 12])
            .style(t::select)
            .menu_style(t::select_menu);
        let encoder = pick_list(ENCODERS.as_slice(), pick(&ENCODERS, &self.cfg.encoder), Msg::SetEncoder)
            .width(300)
            .padding([9, 12])
            .style(t::select)
            .menu_style(t::select_menu);
        let decoder = pick_list(DECODERS.as_slice(), pick(&DECODERS, &self.cfg.decoder), Msg::SetDecoder)
            .width(300)
            .padding([9, 12])
            .style(t::select)
            .menu_style(t::select_menu);
        let port = text_input("7000", &self.port_text).on_input(Msg::PortInput).width(110).padding([9, 12]).style(t::field);

        let sharing = card(
            column![
                card_title("Sharing"),
                setting_row("Quality", "Lower it if the connection is slow or unstable.", quality),
                setting_row("Allow control", "Let the other person use your keyboard and mouse.", switch(self.cfg.allow_control, Msg::Control)),
                setting_row("Show my mouse pointer", "Draw the pointer in the shared video.", switch(self.cfg.cursor, Msg::Cursor)),
                setting_row("Network port (UDP)", "Must be open in your firewall/router for internet use. Restart sharing to apply.", port),
                setting_row("Remembered screen", "Forget which screen or window you shared.", btn("Forget", Some(Msg::ForgetScreen), t::secondary)),
                setting_row("Video encoder", "Automatic picks the fastest one your computer has.", encoder),
            ]
            .spacing(18),
        );
        let viewing = card(
            column![
                card_title("Viewing"),
                setting_row("Start in full screen", "Open remote screens covering the whole display.", switch(self.cfg.fullscreen, Msg::Fullscreen)),
                setting_row("Video decoder", "Automatic picks the fastest one your computer has.", decoder),
            ]
            .spacing(18),
        );
        let status = match &self.update {
            UpdateState::Checking => "Checking…".to_string(),
            UpdateState::UpToDate => format!("Version {VERSION} is the latest."),
            UpdateState::Disabled => "Disabled by LINDESK_NO_UPDATE.".to_string(),
            UpdateState::Available(r) => format!("{} is available.", r.tag),
            UpdateState::Downloading { tag, .. } => format!("Installing {tag}…"),
            UpdateState::Failed(e) => format!("Could not update: {e}"),
        };
        let updates = card(
            column![
                card_title("Updates"),
                setting_row("Update automatically", "Check when lindesk opens and install new versions.", switch(self.cfg.auto_update, Msg::AutoUpdate)),
                setting_row("Status", "", row![text(status).size(13).color(t::MUTED), btn("Check now", Some(Msg::CheckUpdate), t::secondary)].spacing(12).align_y(Alignment::Center)),
            ]
            .spacing(18),
        );
        column![heading("Settings", "Defaults work well. Change them only if you need to."), sharing, viewing, updates].spacing(20).into()
    }
}

/// `LD-ABCDEF-GHIJKL-…-XYZ123`: enough to tell codes apart without showing the secret in full.
fn short_code(code: &str) -> String {
    let parts: Vec<&str> = code.split('-').collect();
    if parts.len() > 3 { format!("{}-{}-…-{}", parts[0], parts[1], parts[parts.len() - 1]) } else { code.to_string() }
}
