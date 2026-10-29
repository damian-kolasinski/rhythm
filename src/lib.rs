//! Find, record, and surface the background jobs agents leave on a machine.
//! launchd runs the jobs; Rhythm observes them.
pub mod bounded_command;
pub mod jobs;
pub mod launchd;
pub mod operation_error;
pub mod remote;
pub mod status;
pub mod status_render;
