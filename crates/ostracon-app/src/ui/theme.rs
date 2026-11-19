//! Colours and sizes for Ostracon's Todoist-like look, in each scheme's light and dark.

use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig, ThemeMode, ThemeRegistry, ThemeSet};
use gpui_kit::*;
use ostracon_core::ProjectColor;
use ostracon_core::settings::{Appearance, Scheme};

use crate::format::Urgency;

/// The UI font, vendored in `assets/fonts` under the SIL Open Font License and built into the binary.
pub const FONT_FAMILY: &str = "Inter";

/// Registers the vendored Inter faces and makes Inter the theme's font, so gpui-component's own text (inputs,
/// menus, tooltips) uses it as well as Ostracon's views.
pub fn install_font(cx: &mut App) {
    let faces: [&'static [u8]; 4] = [
        include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
        include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
        include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
        include_bytes!("../../assets/fonts/Inter-Bold.ttf"),
    ];
    cx.text_system()
        .add_fonts(faces.into_iter().map(Into::into).collect())
        .expect("the vendored Inter fonts load");
    Theme::update(cx, |theme| theme.font_family = FONT_FAMILY.into());
}

/// Task names (and the composer, so text doesn't change size when editing).
pub const TASK_TEXT: f32 = 15.;
pub const META_TEXT: f32 = 12.5;
pub const SIDEBAR_WIDTH: f32 = 280.;
/// The checkbox column: names, dividers and the header all line up with it.
pub const CHECKBOX: f32 = 18.;
/// Room per nesting level.
pub const INDENT: f32 = 28.;

/// What the window is drawn in: a scheme in one of its modes, with the appearance setting resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub scheme: Scheme,
    pub mode: ThemeMode,
}

impl Look {
    /// `appearance` resolved against the window's, which it follows when set to [`Appearance::System`].
    pub fn new(scheme: Scheme, appearance: Appearance, window: WindowAppearance) -> Look {
        let mode = match appearance {
            Appearance::System => window.into(),
            Appearance::Light => ThemeMode::Light,
            Appearance::Dark => ThemeMode::Dark,
        };
        Look { scheme, mode }
    }

    /// Draws every window in this look: Ostracon's own [`Palette`], and gpui-component's theme for its inputs,
    /// menus, buttons and Markdown.
    pub fn apply(self, cx: &mut App) {
        let config = match self.scheme {
            Scheme::Classic => {
                let registry = ThemeRegistry::global(cx);
                match self.mode {
                    ThemeMode::Light => registry.default_light_theme(),
                    ThemeMode::Dark => registry.default_dark_theme(),
                }
                .clone()
            }
            Scheme::Flexoki => flexoki(self.mode),
        };
        cx.set_global(self.palette());
        Theme::update(cx, |theme| theme.apply_config(&config));
    }

    pub fn palette(self) -> Palette {
        let dark = self.mode.is_dark();
        match (self.scheme, self.mode) {
            (Scheme::Classic, ThemeMode::Light) => Palette {
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
                toast: rgb(0x202020).into(),
                toast_text: rgb(0xffffff).into(),
                toast_action: rgb(0xff8a7f).into(),
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
            },
            (Scheme::Classic, ThemeMode::Dark) => Palette {
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
                toast: rgb(0x3a3a3a).into(),
                toast_text: rgb(0xffffff).into(),
                toast_action: rgb(0xff8a7f).into(),
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
            },
            // Flexoki's light theme: the 600 accents on paper.
            (Scheme::Flexoki, ThemeMode::Light) => Palette {
                dark,
                page: rgb(0xfffcf0).into(),
                sidebar: rgb(0xf2f0e5).into(),
                sidebar_hover: rgb(0xe6e4d9).into(),
                sidebar_selected: rgb(0xffe1d5).into(),
                text: rgb(0x100f0f).into(),
                muted: rgb(0x6f6e69).into(),
                faint: rgb(0xb7b5ac).into(),
                hairline: rgb(0xe6e4d9).into(),
                accent: rgb(0xaf3029).into(),
                accent_text: rgb(0xfffcf0).into(),
                row_selected: rgba(0xaf30291a).into(),
                row_hover: rgb(0xf2f0e5).into(),
                card: rgb(0xfffcf0).into(),
                card_border: rgb(0xdad8ce).into(),
                backdrop: rgba(0x100f0f52).into(),
                toast: rgb(0x100f0f).into(),
                toast_text: rgb(0xfffcf0).into(),
                toast_action: rgb(0xd14d41).into(),
                overdue: rgb(0xaf3029).into(),
                today: rgb(0x66800b).into(),
                tomorrow: rgb(0xbc5215).into(),
                soon: rgb(0x5e409d).into(),
                later: rgb(0x878580).into(),
                block_project: rgba(0x205ea626).into(),
                block_due: rgba(0x66800b26).into(),
                block_reminder: rgba(0xbc521526).into(),
                warning_bg: rgb(0xfaeec6).into(),
                warning_text: rgb(0x664d01).into(),
            },
            // Flexoki's dark theme: the 400 accents on black.
            (Scheme::Flexoki, ThemeMode::Dark) => Palette {
                dark,
                page: rgb(0x100f0f).into(),
                sidebar: rgb(0x1c1b1a).into(),
                sidebar_hover: rgb(0x282726).into(),
                sidebar_selected: rgba(0xd14d4133).into(),
                text: rgb(0xcecdc3).into(),
                muted: rgb(0x878580).into(),
                faint: rgb(0x575653).into(),
                hairline: rgb(0x282726).into(),
                accent: rgb(0xd14d41).into(),
                accent_text: rgb(0xfffcf0).into(),
                row_selected: rgba(0xd14d4126).into(),
                row_hover: rgb(0x1c1b1a).into(),
                card: rgb(0x1c1b1a).into(),
                card_border: rgb(0x343331).into(),
                backdrop: rgba(0x0000008c).into(),
                toast: rgb(0x403e3c).into(),
                toast_text: rgb(0xfffcf0).into(),
                toast_action: rgb(0xd14d41).into(),
                overdue: rgb(0xd14d41).into(),
                today: rgb(0x879a39).into(),
                tomorrow: rgb(0xda702c).into(),
                soon: rgb(0x8b7ec8).into(),
                later: rgb(0x878580).into(),
                block_project: rgba(0x4385be33).into(),
                block_due: rgba(0x879a3933).into(),
                block_reminder: rgba(0xda702c33).into(),
                warning_bg: rgb(0x241e08).into(),
                warning_text: rgb(0xd0a215).into(),
            },
        }
    }
}

/// gpui-component's theme for Flexoki in `mode`, from `assets/themes/flexoki.json`.
fn flexoki(mode: ThemeMode) -> Rc<ThemeConfig> {
    let set: ThemeSet = serde_json::from_str(include_str!("../../assets/themes/flexoki.json"))
        .expect("the vendored Flexoki theme parses");
    let config = set
        .themes
        .into_iter()
        .find(|config| config.mode == mode)
        .expect("the vendored Flexoki theme has a light and a dark variant");
    Rc::new(config)
}

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
    pub toast: Hsla,
    pub toast_text: Hsla,
    /// The toast's Undo.
    pub toast_action: Hsla,
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

impl Global for Palette {}

/// The palette of the [`Look`] applied last.
pub fn palette(cx: &App) -> Palette {
    *cx.global::<Palette>()
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
