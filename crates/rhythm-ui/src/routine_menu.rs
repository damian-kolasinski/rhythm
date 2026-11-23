//! The actions a routine's row offers, behind its "More" button or a
//! secondary click anywhere on the row.
use rune::{El, Icon, el};
use rune_kit::{ControlSize, IconButton, IconButtonVariant, Kit, Menu, MenuItem};

use crate::{app::Window, routine::Routine};

/// Three dots.
static MORE: Icon = Icon {
    name: "rhythm-more",
    view: 16.0,
    body: r#"<circle cx="3.5" cy="8" r="1.25" fill="currentColor"/><circle cx="8" cy="8" r="1.25" fill="currentColor"/><circle cx="12.5" cy="8" r="1.25" fill="currentColor"/>"#,
};

/// The width of the column the button sits in.
pub const WIDTH: f32 = 28.0;

/// Where the open menu is: under the row's button, or at the window point a
/// secondary click landed on.
#[derive(Clone, Debug, PartialEq)]
pub struct Open {
    pub key: String,
    pub at: Option<(f32, f32)>,
}

/// The row's button, with its menu when open; nothing for a routine the
/// window cannot change.
pub fn button(state: &Window, routine: &Routine, kit: &Kit, layer: &mut Vec<El>) -> El {
    if routine.removable.is_none() {
        return el().w(WIDTH);
    }
    let open = state.menu.as_ref().filter(|open| open.key == routine.key);
    let key = routine.key.clone();
    let trigger = IconButton::new(&MORE)
        .variant(IconButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .label(format!("Actions for {}", routine.name))
        .on_press(move |window: &mut Window| window.toggle_menu(&key))
        .build(kit)
        .key(format!("{}/more", routine.key));
    let key = routine.key.clone();
    let mut menu = Menu::new(format!("{}/actions", routine.key))
        .trigger(trigger)
        .open(open.is_some())
        .placement(rune_kit::Placement::BottomEnd)
        .on_close(Window::close_menu)
        .item(
            MenuItem::new("Remove…")
                .danger(true)
                .on_select(move |window: &mut Window| window.ask_remove(&key)),
        );
    if let Some((x, y)) = open.and_then(|open| open.at) {
        menu = menu.at(x, y);
    }
    el().w(WIDTH).child(menu.build(kit, layer))
}
