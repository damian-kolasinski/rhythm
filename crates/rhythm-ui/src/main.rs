#[cfg(target_os = "macos")]
mod app;
#[cfg(target_os = "macos")]
mod finder;
#[cfg(target_os = "macos")]
mod problems;
#[cfg(target_os = "macos")]
mod removal;
#[cfg(target_os = "macos")]
mod removal_view;
#[cfg(target_os = "macos")]
mod routine;
#[cfg(target_os = "macos")]
mod routine_control;
#[cfg(target_os = "macos")]
mod routine_menu;
#[cfg(target_os = "macos")]
mod scenes;
#[cfg(target_os = "macos")]
mod session_dialog;
#[cfg(target_os = "macos")]
mod theme;
#[cfg(target_os = "macos")]
mod updates;
#[cfg(target_os = "macos")]
mod view;

#[cfg(target_os = "macos")]
fn main() {
    app::run();
}

#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
    eprintln!("The Rhythm desktop app requires macOS.");
    std::process::ExitCode::FAILURE
}
