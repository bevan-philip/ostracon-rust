//! Colours and sizes for Ostracon's Todoist-like look, in light and dark.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use ostracon_core::ProjectColor;

use crate::format::Urgency;

/// Task names (and the composer, so text doesn't change size when editing).
pub const TASK_TEXT: f32 = 15.;
pub const META_TEXT: f32 = 12.5;
pub const SIDEBAR_WIDTH: f32 = 280.;
/// The checkbox column: names, dividers and the header all line up with it.
pub const CHECKBOX: f32 = 18.;
/// Room per nesting level.
pub const INDENT: f32 = 28.;

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub page: Hsla,
    pub sidebar: Hsla,
    pub sidebar_hover: Hsla,
    pub sidebar_selected: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    pub hairline: Hsla,
    pub accent: Hsla,
    pub accent_text: Hsla,
    pub row_selected: Hsla,
    pub row_hover: Hsla,
    pub card: Hsla,
    pub card_border: Hsla,
    pub backdrop: Hsla,
    pub overdue: Hsla,
    pub today: Hsla,
    pub tomorrow: Hsla,
    pub soon: Hsla,
    pub later: Hsla,
    pub block_project: Hsla,
    pub block_due: Hsla,
    pub block_reminder: Hsla,
    pub warning_bg: Hsla,
    pub warning_text: Hsla,
}

pub fn palette(cx: &App) -> Palette {
    let dark = cx.theme().mode.is_dark();
    if dark {
        Palette {
            dark,
            page: rgb(0x1e1e1e).into(),
            sidebar: rgb(0x262626).into(),
            sidebar_hover: rgb(0x2f2f2f).into(),
            sidebar_selected: rgba(0xdc4c3e33).into(),
            text: rgb(0xe6e6e6).into(),
            muted: rgb(0x9a9a9a).into(),
            faint: rgb(0x6a6a6a).into(),
            hairline: rgb(0x333333).into(),
            accent: rgb(0xe0574a).into(),
            accent_text: rgb(0xffffff).into(),
            row_selected: rgba(0xdc4c3e26).into(),
            row_hover: rgb(0x262626).into(),
            card: rgb(0x282828).into(),
            card_border: rgb(0x3a3a3a).into(),
            backdrop: rgba(0x0000008c).into(),
            overdue: rgb(0xf0645a).into(),
            today: rgb(0x3fb55b).into(),
            tomorrow: rgb(0xe08a2e).into(),
            soon: rgb(0xa77ef0).into(),
            later: rgb(0x9a9a9a).into(),
            block_project: rgba(0x4180ff33).into(),
            block_due: rgba(0x3fb55b33).into(),
            block_reminder: rgba(0xe08a2e33).into(),
            warning_bg: rgb(0x3a2f1a).into(),
            warning_text: rgb(0xf2c46d).into(),
        }
    } else {
        Palette {
            dark,
            page: rgb(0xffffff).into(),
            sidebar: rgb(0xfcfaf8).into(),
            sidebar_hover: rgb(0xf2efed).into(),
            sidebar_selected: rgb(0xffefe5).into(),
            text: rgb(0x202020).into(),
            muted: rgb(0x666666).into(),
            faint: rgb(0x999999).into(),
            hairline: rgb(0xf0f0f0).into(),
            accent: rgb(0xdc4c3e).into(),
            accent_text: rgb(0xffffff).into(),
            row_selected: rgb(0xfff4ef).into(),
            row_hover: rgb(0xfafafa).into(),
            card: rgb(0xffffff).into(),
            card_border: rgb(0xe6e6e6).into(),
            backdrop: rgba(0x00000052).into(),
            overdue: rgb(0xd1453b).into(),
            today: rgb(0x058527).into(),
            tomorrow: rgb(0xad6200).into(),
            soon: rgb(0x692fc2).into(),
            later: rgb(0x808080).into(),
            block_project: rgba(0x4180ff26).into(),
            block_due: rgba(0x05852726).into(),
            block_reminder: rgba(0xad620026).into(),
            warning_bg: rgb(0xfff6e0).into(),
            warning_text: rgb(0x7a5200).into(),
        }
    }
}

impl Palette {
    pub fn urgency(&self, urgency: Urgency) -> Hsla {
        match urgency {
            Urgency::Overdue => self.overdue,
            Urgency::Today => self.today,
            Urgency::Tomorrow => self.tomorrow,
            Urgency::Soon => self.soon,
            Urgency::Later => self.later,
        }
    }

    /// The page, lightly tinted with the project's colour.
    pub fn tinted_page(&self, color: ProjectColor) -> Hsla {
        let tint = project_color(color);
        mix(self.page, tint, if self.dark { 0.05 } else { 0.035 })
    }
}

pub fn project_color(color: ProjectColor) -> Hsla {
    rgb(color.rgb()).into()
}

/// `a` moved `amount` of the way towards `b`.
pub fn mix(a: Hsla, b: Hsla, amount: f32) -> Hsla {
    let (a, b) = (a.to_rgb(), b.to_rgb());
    let lerp = |x: f32, y: f32| x + (y - x) * amount;
    Rgba {
        r: lerp(a.r, b.r),
        g: lerp(a.g, b.g),
        b: lerp(a.b, b.b),
        a: 1.,
    }
    .into()
}
