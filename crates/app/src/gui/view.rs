//! Layout of every screen.

use iced::widget::{Column, Space, button, column, container, pick_list, progress_bar, row, scrollable, stack, text, text_input, toggler};
use iced::{Alignment, Color, Element, Length};

use super::theme as t;
use super::{App, HostState, Kind2, Msg, Page, UpdateState, VERSION, ViewerState};
use crate::config::{Lang, Quality};
use crate::i18n::Tx;
use crate::netinfo::Kind;
use lindesk_proto::id;

/// Encoder/decoder choice shown in a dropdown.
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QOpt(pub Quality, pub &'static str);

impl std::fmt::Display for QOpt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.1)
    }
}

fn encoders(x: Tx) -> Vec<Opt> {
    vec![
        Opt { value: "auto", label: x.auto_label() },
        Opt { value: "va", label: x.va_label() },
        Opt { value: "nv", label: x.nv_label(false) },
        Opt { value: "x264", label: x.cpu_label() },
    ]
}

fn decoders(x: Tx) -> Vec<Opt> {
    vec![
        Opt { value: "auto", label: x.auto_label() },
        Opt { value: "va", label: x.va_label() },
        Opt { value: "nv", label: x.nv_label(true) },
        Opt { value: "sw", label: x.cpu_label() },
    ]
}

fn current(list: &[Opt], value: &str) -> Option<Opt> {
    list.iter().find(|o| o.value == value).cloned()
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

fn btn<'a>(label: impl text::IntoFragment<'a>, msg: Option<Msg>, style: fn(&iced::Theme, button::Status) -> button::Style) -> Element<'a, Msg> {
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

fn banner<'a>(kind: Kind2, msg: &'a str, close: &'a str) -> Element<'a, Msg> {
    let c = if kind == Kind2::Error { t::BAD } else { t::GOOD };
    container(
        row![text(msg).size(14).width(Length::Fill), button(text(close).size(13)).style(t::link).on_press(Msg::DismissNotice)]
            .spacing(12)
            .align_y(Alignment::Center),
    )
    .padding([12, 16])
    .width(Length::Fill)
    .style(t::tint(c))
    .into()
}

fn centered<'a>(panel: Element<'a, Msg>) -> Element<'a, Msg> {
    container(panel).center(Length::Fill).style(t::scrim).into()
}

// --- screens ------------------------------------------------------------------

impl App {
    pub fn view(&self) -> Element<'_, Msg> {
        let x = self.tx();
        let body = match self.page {
            Page::Share => self.share_page(x),
            Page::Connect => self.connect_page(x),
            Page::Settings => self.settings_page(x),
        };
        let mut content = Column::new().spacing(20).max_width(760);
        if let Some((kind, msg)) = &self.notice {
            content = content.push(banner(*kind, msg, x.dismiss()));
        }
        content = content.push(body);
        let main = container(scrollable(container(content).padding(32).center_x(Length::Fill)).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(t::page);
        let mut out: Element<'_, Msg> = row![self.sidebar(x), main].into();

        if let Some(req) = &self.request {
            let panel = card(
                column![
                    text(x.req_title(&req.name)).size(22).font(t::BOLD),
                    text(x.req_body(self.cfg.allow_control)).size(14),
                    container(text(x.req_code(&req.verification)).size(18).font(t::MONO)).padding([10, 14]).width(Length::Fill).style(t::code_box),
                    hint(x.req_code_hint()),
                    row![
                        btn(x.accept(), Some(Msg::Accept(false)), t::primary),
                        btn(x.always_allow(), Some(Msg::Accept(true)), t::secondary),
                        btn(x.reject(), Some(Msg::Reject), t::danger),
                    ]
                    .spacing(8),
                ]
                .spacing(14)
                .width(460),
            );
            out = stack![out, centered(panel)].into();
        }
        if let UpdateState::Downloading { tag, progress } = &self.update {
            let panel = card(
                column![
                    text(x.upd_overlay_title(tag)).size(20).font(t::BOLD),
                    progress_bar(0.0..=1.0, *progress).girth(8),
                    hint(x.upd_overlay_hint()),
                ]
                .spacing(16)
                .width(340),
            );
            out = stack![out, centered(panel)].into();
        }
        out
    }

    fn sidebar(&self, x: Tx) -> Element<'_, Msg> {
        let item = |label: &'static str, page: Page| -> Element<'_, Msg> {
            button(text(label).size(14).font(t::SEMIBOLD).width(Length::Fill))
                .padding([10, 14])
                .width(Length::Fill)
                .style(t::nav(self.page == page))
                .on_press(Msg::Nav(page))
                .into()
        };
        let status = match &self.update {
            UpdateState::Checking => hint(x.upd_checking()),
            UpdateState::UpToDate => hint(x.upd_latest()),
            UpdateState::Disabled => hint(x.upd_disabled()),
            UpdateState::Failed(_) => hint(x.upd_check_failed()),
            UpdateState::Downloading { .. } => hint(x.upd_updating()),
            UpdateState::Available(r) => column![
                text(x.upd_available(&r.tag)).size(13).color(t::GOOD),
                button(text(x.upd_now()).size(13)).style(t::link).padding(0).on_press(Msg::InstallUpdate)
            ]
            .spacing(2)
            .into(),
        };
        let nav = column![item(x.nav_share(), Page::Share), item(x.nav_connect(), Page::Connect), item(x.nav_settings(), Page::Settings)].spacing(4);
        let brand = row![
            iced::widget::image(iced::widget::image::Handle::from_bytes(include_bytes!("../../assets/icon.png").as_slice())).width(34).height(34),
            column![text("lindesk").size(20).font(t::BOLD), text(x.tagline()).size(12).color(t::MUTED)].spacing(0)
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        container(column![brand, Space::new().height(12), nav, Space::new().height(Length::Fill), hint(x.version(VERSION)), status].spacing(8))
            .padding(20)
            .width(230)
            .height(Length::Fill)
            .style(t::sidebar)
            .into()
    }

    fn share_page(&self, x: Tx) -> Element<'_, Msg> {
        let (dot_color, title, sub) = match &self.host {
            HostState::Stopped => {
                let (a, b) = x.st_off();
                (t::MUTED, a.to_string(), b.to_string())
            }
            HostState::Starting => {
                let (a, b) = x.st_starting();
                (t::WARN, a.to_string(), b.to_string())
            }
            HostState::Waiting { encoder } => {
                let (a, b) = x.st_waiting(encoder);
                (t::WARN, a.to_string(), b)
            }
            HostState::Connected { name, fps, kbps } => {
                let (a, b) = x.st_connected(name, *fps, *kbps as f32 / 1000.0);
                (t::GOOD, a, b)
            }
        };
        let running = !matches!(self.host, HostState::Stopped);
        let action = if running { btn(x.stop_sharing(), Some(Msg::StopHost), t::danger) } else { btn(x.start_sharing(), Some(Msg::StartHost), t::primary) };
        let status = card(
            row![
                dot(dot_color),
                column![text(title).size(18).font(t::SEMIBOLD), text(sub).size(14).color(t::MUTED)].spacing(2).width(Length::Fill),
                action
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        );

        let id_text = self.my_id().map(|i| id::pretty(&i)).unwrap_or_else(|| x.no_network_id().to_string());
        let copy = btn(if self.copied_at.is_some() { x.copied() } else { x.copy_id() }, self.my_id().map(|_| Msg::CopyId), t::primary);
        let id_card = card(
            column![
                card_title(x.id_title()),
                row![text(id_text).size(48).font(t::BOLD).color(t::ACCENT).width(Length::Fill), copy].align_y(Alignment::Center).spacing(12),
                hint(x.id_hint()),
                hint(x.id_changes()),
            ]
            .spacing(12),
        );

        let route = self.route();
        let (reach_color, reach_text) = match route {
            Some(r) => (if matches!(r.kind, Kind::Vpn | Kind::Lan) { t::GOOD } else { t::WARN }, x.reach(Some(r.kind), &r.iface)),
            None => (t::BAD, x.reach(None, "")),
        };
        let mut reach = Column::new().spacing(12).push(card_title(x.reach_title())).push(
            row![dot(reach_color), text(reach_text).size(14).width(Length::Fill)].spacing(10).align_y(Alignment::Start),
        );
        reach = reach.push(hint(x.port_note(self.cfg.port)));
        reach = reach.push(button(text(x.other_network()).size(13)).style(t::link).padding(0).on_press(Msg::ToggleRoutes));
        if self.show_routes {
            let pick = pick_list(self.routes.clone(), route.cloned(), Msg::PickRoute)
                .placeholder("—")
                .width(Length::Fill)
                .padding([9, 12])
                .style(t::select)
                .menu_style(t::select_menu);
            let detect = if self.public_busy { btn(x.detecting(), None, t::secondary) } else { btn(x.detect_public(), Some(Msg::DetectPublic), t::secondary) };
            reach = reach.push(row![pick, detect].spacing(8));
        }

        let mut page = Column::new().spacing(20).push(heading(x.share_title(), x.share_sub()));
        if !self.missing.is_empty() {
            page = page.push(self.missing_banner(x));
        }
        page = page.push(status);
        if !running {
            page = page.push(hint(x.first_time_hint()));
        }
        page.push(id_card).push(card(reach)).into()
    }

    fn missing_banner(&self, x: Tx) -> Element<'_, Msg> {
        let mut c = Column::new().spacing(4).push(text(x.missing_title()).size(14).font(t::SEMIBOLD));
        for m in &self.missing {
            c = c.push(text(format!("• {m}")).size(13));
        }
        c = c.push(text(x.missing_footer()).size(13));
        container(c).padding([12, 16]).width(Length::Fill).style(t::tint(t::WARN)).into()
    }

    fn connect_page(&self, x: Tx) -> Element<'_, Msg> {
        let idle = matches!(self.viewer, ViewerState::Idle);
        let valid = id::parse(&self.id_input).is_ok();
        let input = text_input(x.id_placeholder(), &self.id_input)
            .on_input(Msg::IdInput)
            .on_submit_maybe(valid.then_some(Msg::Connect))
            .padding([11, 14])
            .size(18)
            .font(t::MONO)
            .style(t::field);
        let go = btn(x.connect(), (idle && valid).then_some(Msg::Connect), t::primary);
        let entry = card(column![card_title(x.conn_card()), hint(x.conn_hint()), row![input, go].spacing(8).align_y(Alignment::Center)].spacing(12));

        let session: Option<Element<'_, Msg>> = match &self.viewer {
            ViewerState::Idle => None,
            ViewerState::Connecting => {
                let (a, b) = x.sess_connecting();
                Some(self.session_card(t::WARN, a.into(), b.into(), x))
            }
            ViewerState::Waiting { verification } => {
                let (a, b) = x.sess_waiting(verification);
                Some(self.session_card(t::WARN, a.into(), b, x))
            }
            ViewerState::Connected { fps, rtt_ms } => {
                let (a, b) = x.sess_connected(*fps, *rtt_ms);
                Some(self.session_card(t::GOOD, a.into(), b, x))
            }
        };

        let mut recents = Column::new().spacing(8).push(card_title(x.recent_title()));
        if self.cfg.recent.is_empty() {
            recents = recents.push(hint(x.recent_empty()));
        }
        for (i, r) in self.cfg.recent.iter().enumerate() {
            let line = row![
                text(id::pretty(&r.id)).size(16).font(t::MONO).width(Length::Fill),
                btn(x.connect(), idle.then_some(Msg::ConnectRecent(i)), t::secondary),
                button(text(x.remove()).size(13)).style(t::link).on_press(Msg::RemoveRecent(i)),
            ]
            .spacing(10)
            .align_y(Alignment::Center);
            recents = recents.push(container(line).padding([10, 14]).width(Length::Fill).style(t::code_box));
        }

        let mut page = Column::new().spacing(20).push(heading(x.conn_title(), x.conn_sub()));
        if !self.missing.is_empty() {
            page = page.push(self.missing_banner(x));
        }
        page = page.push(entry);
        if let Some(s) = session {
            page = page.push(s);
        }
        page.push(card(recents)).into()
    }

    fn session_card(&self, c: Color, title: String, sub: String, x: Tx) -> Element<'_, Msg> {
        card(
            row![
                dot(c),
                column![text(title).size(18).font(t::SEMIBOLD), text(sub).size(14).color(t::MUTED)].spacing(2).width(Length::Fill),
                btn(x.disconnect(), Some(Msg::Disconnect), t::danger)
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        )
    }

    fn settings_page(&self, x: Tx) -> Element<'_, Msg> {
        let lang = pick_list(Lang::ALL.to_vec(), Some(self.cfg.lang), Msg::SetLang).width(300).padding([9, 12]).style(t::select).menu_style(t::select_menu);
        let qualities: Vec<QOpt> = Quality::ALL.iter().map(|q| QOpt(*q, x.quality_name(*q))).collect();
        let quality = pick_list(qualities, Some(QOpt(self.cfg.quality, x.quality_name(self.cfg.quality))), Msg::SetQuality)
            .width(300)
            .padding([9, 12])
            .style(t::select)
            .menu_style(t::select_menu);
        let encs = encoders(x);
        let encoder = pick_list(encs.clone(), current(&encs, &self.cfg.encoder), Msg::SetEncoder).width(300).padding([9, 12]).style(t::select).menu_style(t::select_menu);
        let decs = decoders(x);
        let decoder = pick_list(decs.clone(), current(&decs, &self.cfg.decoder), Msg::SetDecoder).width(300).padding([9, 12]).style(t::select).menu_style(t::select_menu);
        let port = text_input("7000", &self.port_text).on_input(Msg::PortInput).width(110).padding([9, 12]).style(t::field);

        let general = card(column![card_title(x.set_general()), setting_row(x.language().0, x.language().1, lang)].spacing(18));
        let sharing = card(
            column![
                card_title(x.set_sharing()),
                setting_row(x.quality().0, x.quality().1, quality),
                setting_row(x.allow_control().0, x.allow_control().1, switch(self.cfg.allow_control, Msg::Control)),
                setting_row(x.show_cursor().0, x.show_cursor().1, switch(self.cfg.cursor, Msg::Cursor)),
                setting_row(x.port().0, x.port().1, port),
                setting_row(x.remembered_screen().0, x.remembered_screen().1, btn(x.forget(), Some(Msg::ForgetScreen), t::secondary)),
                setting_row(x.encoder().0, x.encoder().1, encoder),
            ]
            .spacing(18),
        );

        let mut trusted = Column::new().spacing(8).push(card_title(x.trusted_title())).push(hint(x.trusted_hint()));
        if self.cfg.trusted.is_empty() {
            trusted = trusted.push(hint(x.trusted_empty()));
        }
        for (i, tr) in self.cfg.trusted.iter().enumerate() {
            let fp = lindesk_proto::session::from_hex32(&tr.key).map(|k| lindesk_proto::session::fingerprint(&k)).unwrap_or_default();
            let line = row![
                column![text(&tr.name).size(14).font(t::SEMIBOLD), text(fp).size(12).font(t::MONO).color(t::MUTED)].spacing(2).width(Length::Fill),
                button(text(x.remove()).size(13)).style(t::link).on_press(Msg::RemoveTrusted(i)),
            ]
            .align_y(Alignment::Center);
            trusted = trusted.push(container(line).padding([10, 14]).width(Length::Fill).style(t::code_box));
        }

        let viewing = card(
            column![
                card_title(x.set_viewing()),
                setting_row(x.fullscreen().0, x.fullscreen().1, switch(self.cfg.fullscreen, Msg::Fullscreen)),
                setting_row(x.decoder().0, x.decoder().1, decoder),
            ]
            .spacing(18),
        );
        let status = match &self.update {
            UpdateState::Checking => x.upd_status_checking().to_string(),
            UpdateState::UpToDate => x.upd_status_latest(VERSION),
            UpdateState::Disabled => x.upd_status_disabled().to_string(),
            UpdateState::Available(r) => x.upd_status_available(&r.tag),
            UpdateState::Downloading { tag, .. } => x.upd_status_installing(tag),
            UpdateState::Failed(e) => x.upd_status_failed(e),
        };
        let updates = card(
            column![
                card_title(x.set_updates()),
                setting_row(x.auto_update().0, x.auto_update().1, switch(self.cfg.auto_update, Msg::AutoUpdate)),
                setting_row(x.status(), "", row![text(status).size(13).color(t::MUTED), btn(x.check_now(), Some(Msg::CheckUpdate), t::secondary)].spacing(12).align_y(Alignment::Center)),
            ]
            .spacing(18),
        );
        column![heading(x.set_title(), x.set_sub()), general, sharing, card(trusted), viewing, updates].spacing(20).into()
    }
}
