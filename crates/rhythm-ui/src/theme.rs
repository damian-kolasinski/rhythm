//! Rhythm's light and dark semantic roles follow the Academy Colors and Web
//! typography frames and the palette of Rhythm's earlier web UI.
use rune::{
    color,
    tok::{Family, TextStyle},
};
use rune_kit::{Kit, StateColors};

pub const FONTS: &[&[u8]] = &[include_bytes!("../assets/fonts/DMSans.ttf")];
pub const GAP: f32 = 8.0;
pub const INSET: f32 = 20.0;
pub const RADIUS: f32 = 7.0;

pub fn heading() -> TextStyle {
    TextStyle::new(23.0, 650.0).lh(28.0).tracking(-0.69)
}
pub fn body() -> TextStyle {
    TextStyle::new(14.0, 400.0).lh(20.0)
}
pub fn strong() -> TextStyle {
    TextStyle::new(14.0, 650.0).lh(20.0)
}
pub fn secondary() -> TextStyle {
    TextStyle::new(12.0, 500.0).lh(16.0)
}
pub fn mono() -> TextStyle {
    TextStyle::new(11.0, 400.0)
        .lh(15.0)
        .family(Family::SystemMono)
}

pub fn kit(dark: bool) -> Kit {
    let mut kit = Kit::neutral(dark);
    let paint = |light, night| color::hex(if dark { night } else { light }).expect("theme color");
    let colors = &mut kit.c;
    colors.page = paint("#f7fcff", "#222526");
    colors.bg_base = colors.page;
    colors.layer_01 = paint("#ebeff2", "#2b2f31");
    colors.surface_base = colors.layer_01;
    colors.surface_hover = paint("#e5f4ff", "#3a3d40");
    colors.border_base = paint("#d2d6d9", "#494d50");
    colors.std_border = paint("#b8bcbf", "#666b6e");
    colors.text_base = paint("#222526", "#ebeff2");
    colors.std_text = colors.text_base;
    colors.text_muted = paint("#525659", "#b8bcbf");
    colors.border_focus = paint("#005999", "#66bfff");
    colors.bg_accent = paint("#003b66", "#66bfff");
    colors.submit_fg = paint("#ffffff", "#052033");
    let state = |bg, fg, border| StateColors {
        bg,
        fg,
        border,
        icon: fg,
    };
    colors.info = state(
        paint("#e5f4ff", "#052033"),
        paint("#003b66", "#99d4ff"),
        colors.border_focus,
    );
    colors.success = state(
        paint("#d8e5cf", "#263421"),
        paint("#264013", "#c6d9b8"),
        paint("#93bf73", "#384d29"),
    );
    colors.danger = state(
        paint("#ffe5e5", "#330808"),
        paint("#732222", "#ffb3b3"),
        paint("#e54545", "#732222"),
    );
    colors.warning = state(
        paint("#fbf1dc", "#3a2f14"),
        paint("#4d390b", "#f2c055"),
        paint("#b28009", "#664700"),
    );
    kit
}
