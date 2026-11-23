//! The confirmation dialog for removing a routine, and the notice once it is
//! gone. Both say where the script the routine ran stays, and show it in
//! Finder.
use std::path::Path;

use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, IconButton, IconButtonVariant, Kit};

use crate::{
    app::Window,
    problems::Room,
    removal::{Flow, Removed, Stage},
    theme,
};

/// The space Kit keeps around a dialog, and the height of the buttons' row.
const MARGIN: f32 = 16.0;
const BUTTONS: f32 = 48.0;

/// The dialog never outgrows the window: what it says scrolls, and its
/// buttons stay in view.
pub fn dialog(flow: &Flow, room: Room, kit: &Kit, layer: &mut Vec<El>) {
    let mut consequence = String::from(
        "Rhythm stops the routine and deletes its launchd definition. Its recorded runs stay in the history.",
    );
    if flow.running {
        consequence.push_str(" It is running now; removing it stops that run.");
    }
    let mut body = col().w_full().px(16.0).gap(12.0).child(
        text(consequence)
            .font(theme::body())
            .color(kit.c.text_base)
            .wrap(),
    );
    if let Some(script) = flow.script() {
        body = body.child(script_warning(script, kit));
    }
    // An error sits next to the button that retries.
    body = match &flow.stage {
        Stage::Reading => body.child(
            text("Checking what the routine runs…")
                .font(theme::secondary())
                .color(kit.c.text_muted)
                .key("reading"),
        ),
        Stage::Unreadable(error) => body.child(callout(
            "Couldn’t read this routine’s definition",
            error.message(),
            kit,
        )),
        Stage::Failed(_, error) => body.child(callout(
            &format!("Couldn’t remove {}", flow.name),
            error.message(),
            kit,
        )),
        Stage::Ready(_) | Stage::Removing(_) => body,
    };
    let (label, ready) = match flow.stage {
        Stage::Ready(_) => ("Remove", true),
        Stage::Reading | Stage::Unreadable(_) => ("Remove", false),
        Stage::Removing(_) => ("Removing…", false),
        Stage::Failed(..) => ("Try again", true),
    };
    let buttons = row()
        .w_full()
        .gap(8.0)
        .padding(16.0)
        .child(el().grow(1.0))
        .child(
            Button::new("Cancel")
                .variant(ButtonVariant::Outline)
                .disabled(!flow.closable())
                .on_press(Window::cancel_remove)
                .build(kit)
                .key("remove-cancel"),
        )
        .child(
            Button::new(label)
                .variant(ButtonVariant::Danger)
                .disabled(!ready)
                .on_press(Window::confirm_remove)
                .build(kit)
                .key("remove-confirm"),
        );
    let room = (room.window - 2.0 * MARGIN - room.header - BUTTONS).clamp(64.0, 420.0);
    let viewport = col()
        .w_full()
        .max_h(room)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .key("remove-details")
        .child(body);
    Dialog::new("remove", format!("Remove {}?", flow.name))
        .description(flow.about.clone())
        .width(460.0)
        .fit()
        .open(true)
        .on_close(Window::cancel_remove)
        .child(kit.scroll_bar(viewport))
        .child(buttons)
        .build(kit, layer);
}

/// Why the dialog cannot go on.
fn callout(title: &str, message: &str, kit: &Kit) -> El {
    let colors = kit.c.danger;
    col()
        .w_full()
        .padding(12.0)
        .gap(2.0)
        .radius(theme::RADIUS)
        .fill(colors.bg)
        .key("remove-error")
        .child(
            text(title.to_owned())
                .font(theme::strong())
                .color(colors.fg),
        )
        .child(
            text(message.to_owned())
                .font(theme::secondary())
                .color(colors.fg)
                .wrap(),
        )
}

/// The routine runs a script, which removal leaves on disk.
fn script_warning(script: &Path, kit: &Kit) -> El {
    let colors = kit.c.warning;
    col()
        .w_full()
        .padding(12.0)
        .gap(4.0)
        .radius(theme::RADIUS)
        .fill(colors.bg)
        .key("script-warning")
        .child(
            text("This routine runs a script")
                .font(theme::strong())
                .color(colors.fg),
        )
        .child(
            text("Removing the routine leaves the script on disk.")
                .font(theme::secondary())
                .color(colors.fg)
                .wrap(),
        )
        .child(script_path(script, kit))
        .child(reveal(script, "reveal-script", kit).mt(4.0))
}

/// Once the routine is gone: what stays, and where.
pub fn notice(removed: &Removed, kit: &Kit) -> El {
    let colors = kit.c.success;
    let mut about = col().grow(1.0).gap(2.0).child(
        text(format!("Removed {}", removed.name))
            .font(theme::strong())
            .color(colors.fg),
    );
    let mut line = row().w_full().padding(12.0).gap(12.0).center_y();
    match &removed.script {
        Some(script) => {
            about = about
                .child(
                    text("Its recorded runs are kept. The script it ran is still on disk:")
                        .font(theme::secondary())
                        .color(colors.fg)
                        .wrap(),
                )
                .child(script_path(script, kit));
            line = line
                .child(about)
                .child(reveal(script, "removed-reveal", kit));
        }
        None => {
            about = about.child(
                text("Its recorded runs are kept.")
                    .font(theme::secondary())
                    .color(colors.fg),
            );
            line = line.child(about);
        }
    }
    line.radius(theme::RADIUS)
        .fill(colors.bg)
        .key("removed")
        .child(
            IconButton::new(kit.glyph.close)
                .variant(IconButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .label("Dismiss")
                .on_press(Window::dismiss_removed)
                .build(kit)
                .key("removed-dismiss"),
        )
}

fn script_path(script: &Path, kit: &Kit) -> El {
    text(rhythm::terminal_text::clean(&script.to_string_lossy()))
        .font(theme::mono())
        .color(kit.c.text_base)
        .truncate_middle()
        .selectable(true)
        .key("script-path")
}

fn reveal(script: &Path, key: &'static str, kit: &Kit) -> El {
    let script = script.to_owned();
    Button::new("Show in Finder")
        .variant(ButtonVariant::Outline)
        .control_size(ControlSize::Small)
        .icon(&rune_kit::glyphs::FOLDER_OPEN)
        .on_press(move |window: &mut Window| window.reveal(&script))
        .build(kit)
        .self_start()
        .key(key)
}
