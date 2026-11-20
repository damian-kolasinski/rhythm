//! Find, record, and surface the background jobs agents leave on a machine.
//! launchd runs the jobs; Rhythm observes them.
pub mod agent_setup;
pub mod agent_setup_render;
pub mod attention;
pub mod attention_render;
pub mod attention_store;
pub mod authoring;
pub mod bounded_command;
mod cron;
mod definition_file;
pub mod demo;
pub mod failure_notice;
pub mod history_render;
#[cfg(target_os = "macos")]
pub mod installation;
pub mod jobs;
pub mod kick;
pub mod kick_render;
pub mod launchd;
pub mod ledger;
pub mod notifier;
pub mod operation_error;
pub mod private_storage;
pub mod recorder;
pub mod recorder_link;
pub mod remote;
pub mod routines;
pub mod run_history;
pub mod run_output;
pub mod runs;
pub mod schedule_input;
pub mod session_start;
pub mod skill_guide;
pub mod state_directory;
pub mod status;
pub mod status_render;
pub mod targets;
pub mod terminal_text;
pub mod time_text;
pub mod wrapper_command;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
