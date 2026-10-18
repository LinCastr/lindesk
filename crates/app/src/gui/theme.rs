//! Visual language: dark, calm surfaces, one accent, 4-pt spacing, rounded cards.

use iced::widget::{button, container, pick_list, text_input};
use iced::{Background, Border, Color, Font, Shadow, Theme, border};

pub const FONT: Font = Font::with_name("Figtree");
pub const BOLD: Font = Font { weight: iced::font::Weight::Bold, ..FONT };
pub const SEMIBOLD: Font = Font { weight: iced::font::Weight::Semibold, ..FONT };
pub const MONO: Font = Font::MONOSPACE;

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

pub const BG: Color = rgb(0x0E, 0x10, 0x15);
pub const SIDEBAR: Color = rgb(0x12, 0x15, 0x1C);
pub const SURFACE: Color = rgb(0x17, 0x1B, 0x23);
pub const ELEVATED: Color = rgb(0x1F, 0x24, 0x2E);
pub const LINE: Color = rgb(0x2A, 0x30, 0x3C);
pub const TEXT: Color = rgb(0xE8, 0xEA, 0xEF);
pub const MUTED: Color = rgb(0x8E, 0x96, 0xA5);
pub const ACCENT: Color = rgb(0x5B, 0x8D, 0xEF);
pub const ON_ACCENT: Color = rgb(0xFF, 0xFF, 0xFF);
pub const GOOD: Color = rgb(0x3D, 0xD6, 0x8C);
pub const WARN: Color = rgb(0xF5, 0xB5, 0x44);
pub const BAD: Color = rgb(0xF0, 0x61, 0x6D);

pub const R: f32 = 10.0;
pub const R_BIG: f32 = 16.0;

pub fn app_theme() -> Theme {
    Theme::custom(
        "lindesk".to_string(),
        iced::theme::Palette { background: BG, text: TEXT, primary: ACCENT, success: GOOD, warning: WARN, danger: BAD },
    )
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgb(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t)
}

fn bordered(color: Color, radius: f32) -> Border {
    Border { color, width: 1.0, radius: border::radius(radius) }
}

// --- containers -------------------------------------------------------------

pub fn page(_: &Theme) -> container::Style {
    container::Style { background: Some(Background::Color(BG)), text_color: Some(TEXT), ..Default::default() }
}

pub fn sidebar(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(SIDEBAR)),
        text_color: Some(TEXT),
        border: Border { color: LINE, width: 0.0, radius: 0.0.into() },
        ..Default::default()
    }
}

pub fn card(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        text_color: Some(TEXT),
        border: bordered(LINE, R_BIG),
        ..Default::default()
    }
}

pub fn code_box(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG)),
        text_color: Some(TEXT),
        border: bordered(LINE, R),
        ..Default::default()
    }
}

/// Tinted pill/banner: a faint wash of `c` with a matching outline.
pub fn tint(c: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(Color { a: 0.12, ..c })),
        text_color: Some(c),
        border: bordered(Color { a: 0.35, ..c }, R),
        ..Default::default()
    }
}

pub fn scrim(_: &Theme) -> container::Style {
    container::Style { background: Some(Background::Color(Color { a: 0.72, ..Color::BLACK })), ..Default::default() }
}

pub fn dot(c: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: Some(Background::Color(c)), border: border::rounded(99), ..Default::default() }
}

// --- buttons ----------------------------------------------------------------

pub fn primary(_: &Theme, s: button::Status) -> button::Style {
    let bg = match s {
        button::Status::Hovered => mix(ACCENT, Color::WHITE, 0.10),
        button::Status::Pressed => mix(ACCENT, Color::BLACK, 0.12),
        button::Status::Disabled => mix(ELEVATED, ACCENT, 0.2),
        button::Status::Active => ACCENT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: if s == button::Status::Disabled { MUTED } else { ON_ACCENT },
        border: border::rounded(R),
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn secondary(_: &Theme, s: button::Status) -> button::Style {
    let bg = match s {
        button::Status::Hovered => mix(ELEVATED, Color::WHITE, 0.05),
        button::Status::Pressed => mix(ELEVATED, Color::BLACK, 0.1),
        _ => ELEVATED,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: if s == button::Status::Disabled { MUTED } else { TEXT },
        border: bordered(LINE, R),
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn danger(_: &Theme, s: button::Status) -> button::Style {
    let bg = match s {
        button::Status::Hovered => mix(BAD, Color::WHITE, 0.10),
        button::Status::Pressed => mix(BAD, Color::BLACK, 0.12),
        _ => BAD,
    };
    button::Style { background: Some(Background::Color(bg)), text_color: Color::WHITE, border: border::rounded(R), shadow: Shadow::default(), snap: true }
}

pub fn nav(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, s| {
        let bg = if selected {
            Some(Background::Color(Color { a: 0.16, ..ACCENT }))
        } else if s == button::Status::Hovered {
            Some(Background::Color(ELEVATED))
        } else {
            None
        };
        button::Style {
            background: bg,
            text_color: if selected { mix(ACCENT, Color::WHITE, 0.35) } else { TEXT },
            border: border::rounded(R),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn link(_: &Theme, s: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: if s == button::Status::Hovered { mix(ACCENT, Color::WHITE, 0.3) } else { ACCENT },
        border: Border::default(),
        shadow: Shadow::default(),
        snap: true,
    }
}

// --- inputs -----------------------------------------------------------------

pub fn field(_: &Theme, s: text_input::Status) -> text_input::Style {
    let edge = match s {
        text_input::Status::Focused { .. } => ACCENT,
        text_input::Status::Hovered => mix(LINE, MUTED, 0.35),
        _ => LINE,
    };
    text_input::Style {
        background: Background::Color(BG),
        border: bordered(edge, R),
        icon: MUTED,
        placeholder: MUTED,
        value: TEXT,
        selection: Color { a: 0.35, ..ACCENT },
    }
}

pub fn select(_: &Theme, s: pick_list::Status) -> pick_list::Style {
    let edge = match s {
        pick_list::Status::Hovered | pick_list::Status::Opened { .. } => mix(LINE, MUTED, 0.35),
        _ => LINE,
    };
    pick_list::Style {
        text_color: TEXT,
        placeholder_color: MUTED,
        handle_color: MUTED,
        background: Background::Color(BG),
        border: bordered(edge, R),
    }
}

pub fn select_menu(_: &Theme) -> iced::overlay::menu::Style {
    iced::overlay::menu::Style {
        background: Background::Color(ELEVATED),
        border: bordered(LINE, R),
        text_color: TEXT,
        selected_text_color: ON_ACCENT,
        selected_background: Background::Color(ACCENT),
        shadow: Shadow { color: Color { a: 0.4, ..Color::BLACK }, offset: iced::Vector::new(0.0, 6.0), blur_radius: 16.0 },
    }
}
