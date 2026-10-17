//! Graphite + honey palette.
use engine::Stratum;
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, rgb};

pub fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub const BG: u32 = 0x0d0e11;
pub const PANEL: u32 = 0x14161b;
pub const RAISED: u32 = 0x1b1e25;
pub const LINE: u32 = 0x262a33;
pub const TEXT: u32 = 0xe8e6e1;
pub const MUTED: u32 = 0x8b909a;
pub const HONEY: u32 = 0xf5a524;
pub const HONEY_DIM: u32 = 0x3a2a0c;
pub const DANGER: u32 = 0xf2555a;

pub fn stratum(s: Stratum) -> Hsla {
    c(match s {
        Stratum::Surface => 0x38bdf8,
        Stratum::Deep => 0xa78bfa,
        Stratum::Buried => 0xc8a27a,
        Stratum::Dark => 0xe11d48,
    })
}

pub fn apply(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    let t = Theme::global_mut(cx);
    let k = &mut t.colors;
    k.background = c(BG);
    k.foreground = c(TEXT);
    k.border = c(LINE);
    k.muted = c(RAISED);
    k.muted_foreground = c(MUTED);
    k.primary = c(HONEY);
    k.primary_hover = c(0xffb83d);
    k.primary_active = c(0xd98c10);
    k.primary_foreground = c(0x1a1206);
    k.button_primary = c(HONEY);
    k.button_primary_hover = c(0xffb83d);
    k.button_primary_active = c(0xd98c10);
    k.button_primary_foreground = c(0x1a1206);
    k.input = c(LINE);
    k.ring = c(HONEY);
    k.caret = c(HONEY);
    k.selection = c(HONEY_DIM);
    k.secondary = c(RAISED);
    k.secondary_hover = c(LINE);
    k.tab_bar = c(PANEL);
    k.tab = c(PANEL);
    k.tab_active = c(RAISED);
    k.tab_active_foreground = c(TEXT);
    k.tab_foreground = c(MUTED);
    k.link = c(HONEY);
    k.popover = c(RAISED);
    k.scrollbar_thumb = c(LINE);
    // Components read the resolved token set, so rebuild it from the edited colors.
    t.tokens = (&t.colors).into();
    Theme::sync_base(cx);
}
