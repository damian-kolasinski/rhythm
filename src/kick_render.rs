//! Terminal rendering of a started job.
use crate::{kick::KickResponse, runs::RunState, time_text};

pub fn render(kicked: &KickResponse) -> String {
    let Some(finished) = &kicked.finished else {
        return format!("Started {}.\n", kicked.label);
    };
    let mut out = String::new();
    if let Some(output) = &finished.output {
        if finished.truncated {
            out.push_str("…\n");
        }
        out.push_str(output);
        if !output.is_empty() && !output.ends_with('\n') {
            out.push('\n');
        }
    }
    let result = finished
        .exit
        .map_or_else(|| "no exit status".to_owned(), |e| e.to_string());
    let detail = match &finished.run {
        Some(run) => match &run.state {
            RunState::Finished { ended_at, .. } => format!(
                " (run {}, {})",
                run.id,
                time_text::duration(ended_at.0 - run.started_at.0)
            ),
            _ => format!(" (run {})", run.id),
        },
        None => String::new(),
    };
    out.push_str(&format!("{} finished: {result}{detail}\n", kicked.label));
    out
}
