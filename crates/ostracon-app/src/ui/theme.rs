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
        let dark = self.mode.is_dark();
        let light_or_dark = if dark { "Dark" } else { "Light" };
        let config = match self.scheme {
            Scheme::Classic => {
                let registry = ThemeRegistry::global(cx);
                match self.mode {
                    ThemeMode::Light => registry.default_light_theme(),
                    ThemeMode::Dark => registry.default_dark_theme(),
                }
                .clone()
            }
            Scheme::Flexoki => vendored(
                include_str!("../../assets/themes/flexoki.json"),
                &format!("Flexoki {light_or_dark}"),
            ),
            Scheme::Merino => vendored(
                include_str!("../../assets/themes/merino.json"),
                &format!("Merino {light_or_dark}"),
            ),
            Scheme::MadoromiHaru => Season::Haru.theme(dark),
            Scheme::MadoromiNatsu => Season::Natsu.theme(dark),
            Scheme::MadoromiAki => Season::Aki.theme(dark),
            Scheme::MadoromiFuyu => Season::Fuyu.theme(dark),
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
            // Merino Light: white panes on near-white, GitHub's blue.
            (Scheme::Merino, ThemeMode::Light) => Palette {
                dark,
                page: rgb(0xffffff).into(),
                sidebar: rgb(0xfafafa).into(),
                sidebar_hover: rgb(0xefeff2).into(),
                sidebar_selected: rgb(0xe5e7eb).into(),
                text: rgb(0x181c21).into(),
                muted: rgb(0x4b535e).into(),
                faint: rgb(0x7a828c).into(),
                hairline: rgb(0xeaecef).into(),
                accent: rgb(0x0969da).into(),
                accent_text: rgb(0xffffff).into(),
                row_selected: rgb(0xeaf3ff).into(),
                row_hover: rgb(0xf5f6f8).into(),
                card: rgb(0xffffff).into(),
                card_border: rgb(0xd8dee4).into(),
                backdrop: rgba(0x181c2152).into(),
                toast: rgb(0x181c21).into(),
                toast_text: rgb(0xffffff).into(),
                toast_action: rgb(0x58a6ff).into(),
                overdue: rgb(0xcf222e).into(),
                today: rgb(0x1a7f37).into(),
                tomorrow: rgb(0x9d5c00).into(),
                soon: rgb(0x8250df).into(),
                later: rgb(0x7a828c).into(),
                block_project: rgba(0x0969da26).into(),
                block_due: rgba(0x1a7f3726).into(),
                block_reminder: rgba(0x9d5c0026).into(),
                warning_bg: rgb(0xfff8c5).into(),
                warning_text: rgb(0x7d4800).into(),
            },
            // Merino Dark: a slate pane in an obsidian shell.
            (Scheme::Merino, ThemeMode::Dark) => Palette {
                dark,
                page: rgb(0x1b1f23).into(),
                sidebar: rgb(0x0f1114).into(),
                sidebar_hover: rgb(0x1a1e24).into(),
                sidebar_selected: rgb(0x2d3136).into(),
                text: rgb(0xe6edf3).into(),
                muted: rgb(0x8b949e).into(),
                faint: rgb(0x6e7681).into(),
                hairline: rgb(0x262c34).into(),
                accent: rgb(0x2f81f7).into(),
                accent_text: rgb(0xffffff).into(),
                row_selected: rgb(0x15253b).into(),
                row_hover: rgb(0x21262d).into(),
                card: rgb(0x21262d).into(),
                card_border: rgb(0x30363d).into(),
                backdrop: rgba(0x0000008c).into(),
                toast: rgb(0x2d333b).into(),
                toast_text: rgb(0xe6edf3).into(),
                toast_action: rgb(0x58a6ff).into(),
                overdue: rgb(0xff7b72).into(),
                today: rgb(0x3fb950).into(),
                tomorrow: rgb(0xd29922).into(),
                soon: rgb(0xbc8cff).into(),
                later: rgb(0x8b949e).into(),
                block_project: rgba(0x2f81f733).into(),
                block_due: rgba(0x3fb95033).into(),
                block_reminder: rgba(0xd2992233).into(),
                warning_bg: rgb(0x332614).into(),
                warning_text: rgb(0xe3b341).into(),
            },
            (Scheme::MadoromiHaru, _) => Inks::of(Season::Haru, dark).palette(dark),
            (Scheme::MadoromiNatsu, _) => Inks::of(Season::Natsu, dark).palette(dark),
            (Scheme::MadoromiAki, _) => Inks::of(Season::Aki, dark).palette(dark),
            (Scheme::MadoromiFuyu, _) => Inks::of(Season::Fuyu, dark).palette(dark),
        }
    }
}

/// The theme called `name` in a vendored gpui-component theme set.
fn vendored(json: &str, name: &str) -> Rc<ThemeConfig> {
    let set: ThemeSet = serde_json::from_str(json).expect("the vendored themes parse");
    let config = set
        .themes
        .into_iter()
        .find(|config| config.name == name)
        .unwrap_or_else(|| panic!("the vendored themes include {name}"));
    Rc::new(config)
}

/// The four seasons of madoromi, each its own [`Scheme`].
#[derive(Clone, Copy)]
enum Season {
    Haru,
    Natsu,
    Aki,
    Fuyu,
}

impl Season {
    /// gpui-component's theme for this season by night when `dark`, else by day.
    fn theme(self, dark: bool) -> Rc<ThemeConfig> {
        let season = match self {
            Season::Haru => "Haru",
            Season::Natsu => "Natsu",
            Season::Aki => "Aki",
            Season::Fuyu => "Fuyu",
        };
        let time = if dark { "Yoru" } else { "Hiru" };
        vendored(
            include_str!("../../assets/themes/madoromi.json"),
            &format!("Madoromi {season} {time}"),
        )
    }
}

/// The madoromi colours Ostracon draws with, under madoromi's own names; its style guide says what each is for.
struct Inks {
    /// Paper: the background.
    kami: u32,
    /// Secondary panes: sidebars and floating windows.
    hyoshi: u32,
    /// Surfaces: hovered and selected items, borders.
    nijimi0: u32,
    nijimi1: u32,
    /// Text.
    sumi: u32,
    /// Secondary text.
    usuzumi0: u32,
    /// Placeholders.
    kasure1: u32,
    /// The accent.
    fuji: u32,
    /// Errors.
    beni: u32,
    /// Success.
    wakaba: u32,
    kaki: u32,
    /// Warnings.
    yamabuki: u32,
    ai: u32,
}

impl Inks {
    /// `season` by night when `dark`, else by day; from madoromi's `palettes/madoromi.json`.
    #[rustfmt::skip]
    fn of(season: Season, dark: bool) -> Inks {
        match (season, dark) {
            (Season::Haru, false) => Inks {
                kami: 0xf6eff8, hyoshi: 0xece5ef, nijimi0: 0xd9ccdd, nijimi1: 0xc8bccd, sumi: 0x584a5c,
                usuzumi0: 0x77697c, kasure1: 0x998d9e, fuji: 0x6928bd, beni: 0xce2700, wakaba: 0x5e8933,
                kaki: 0xfd5217, yamabuki: 0xbda343, ai: 0x1955bf,
            },
            (Season::Haru, true) => Inks {
                kami: 0x241c26, hyoshi: 0x1c151f, nijimi0: 0x392e3c, nijimi1: 0x4e4352, sumi: 0xe1d1e7,
                usuzumi0: 0xb7a8bd, kasure1: 0x8a7e8f, fuji: 0xddb3ff, beni: 0xff8b79, wakaba: 0xc1dd4a,
                kaki: 0xffbb74, yamabuki: 0xffc849, ai: 0x6eadff,
            },
            (Season::Natsu, false) => Inks {
                kami: 0xe9f4f8, hyoshi: 0xddeaf0, nijimi0: 0xbed6df, nijimi1: 0xaec5ce, sumi: 0x3b545e,
                usuzumi0: 0x57717a, kasure1: 0x80969f, fuji: 0x6526b9, beni: 0xb53753, wakaba: 0x00915c,
                kaki: 0xff5735, yamabuki: 0xe3940b, ai: 0x526ccb,
            },
            (Season::Natsu, true) => Inks {
                kami: 0x132227, hyoshi: 0x0e1a1f, nijimi0: 0x23363d, nijimi1: 0x374b53, sumi: 0xc0dde8,
                usuzumi0: 0x97b3be, kasure1: 0x728790, fuji: 0xaea8ff, beni: 0xff8c9f, wakaba: 0xb9d740,
                kaki: 0xffb66d, yamabuki: 0xeec307, ai: 0x4ab8ff,
            },
            (Season::Aki, false) => Inks {
                kami: 0xf8f0e9, hyoshi: 0xf0e6dd, nijimi0: 0xdfcebf, nijimi1: 0xcebeaf, sumi: 0x5d4c3c,
                usuzumi0: 0x7d6b5a, kasure1: 0x9f8f81, fuji: 0x581ca7, beni: 0xa43b3b, wakaba: 0x7e8418,
                kaki: 0xff5635, yamabuki: 0xe3940b, ai: 0x384dad,
            },
            (Season::Aki, true) => Inks {
                kami: 0x271d14, hyoshi: 0x20160e, nijimi0: 0x3d3024, nijimi1: 0x534538, sumi: 0xe8d3c1,
                usuzumi0: 0xbeaa98, kasure1: 0x908072, fuji: 0xd3a1ff, beni: 0xff8672, wakaba: 0xbac105,
                kaki: 0xff9b8f, yamabuki: 0xffae1e, ai: 0x7f9bea,
            },
            (Season::Fuyu, false) => Inks {
                kami: 0xf0f1f8, hyoshi: 0xe5e7f3, nijimi0: 0xccd0e4, nijimi1: 0xbcc0d3, sumi: 0x4a4e63,
                usuzumi0: 0x696d83, kasure1: 0x8d91a4, fuji: 0x5e21b0, beni: 0xbb355a, wakaba: 0x618c32,
                kaki: 0xff5635, yamabuki: 0xe3940b, ai: 0x4a63c3,
            },
            (Season::Fuyu, true) => Inks {
                kami: 0x1c1e2a, hyoshi: 0x151722, nijimi0: 0x2f3241, nijimi1: 0x434757, sumi: 0xd2d6eb,
                usuzumi0: 0xa8adc4, kasure1: 0x7e8294, fuji: 0xbbb8ff, beni: 0xff8799, wakaba: 0xaece34,
                kaki: 0xff9b8f, yamabuki: 0xecba09, ai: 0x829eeb,
            },
        }
    }

    fn palette(&self, dark: bool) -> Palette {
        let solid = |hex: u32| Hsla::from(rgb(hex));
        let wash = |hex: u32, alpha: u32| Hsla::from(rgba(hex << 8 | alpha));
        let tint = if dark { 0x33 } else { 0x26 };
        let warning = solid(self.yamabuki);
        Palette {
            dark,
            page: solid(self.kami),
            sidebar: solid(self.hyoshi),
            sidebar_hover: solid(self.nijimi0),
            sidebar_selected: wash(self.fuji, tint),
            text: solid(self.sumi),
            muted: solid(self.usuzumi0),
            faint: solid(self.kasure1),
            hairline: solid(self.nijimi0),
            accent: solid(self.fuji),
            accent_text: solid(self.kami),
            row_selected: wash(self.fuji, if dark { 0x26 } else { 0x1a }),
            row_hover: solid(self.hyoshi),
            card: solid(self.hyoshi),
            card_border: solid(self.nijimi1),
            backdrop: if dark {
                rgba(0x0000008c).into()
            } else {
                wash(self.sumi, 0x52)
            },
            // A pale slip of paper by day, a darker one by night, so the accent reads on it either way.
            toast: solid(if dark { self.nijimi1 } else { self.nijimi0 }),
            toast_text: solid(self.sumi),
            toast_action: solid(self.fuji),
            overdue: solid(self.beni),
            today: solid(self.wakaba),
            tomorrow: solid(self.kaki),
            soon: solid(self.fuji),
            later: solid(self.usuzumi0),
            block_project: wash(self.ai, tint),
            block_due: wash(self.wakaba, tint),
            block_reminder: wash(self.kaki, tint),
            warning_bg: mix(solid(self.kami), warning, 0.15),
            // By day yamabuki is too pale to read as text, so it is darkened towards the ink.
            warning_text: if dark {
                warning
            } else {
                mix(warning, solid(self.sumi), 0.5)
            },
        }
    }
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
