//! Session activity and outcome for a recorded run.
use rhythm::{
    jobs::RunStore,
    operation_error::{Error, Result},
    runs::Timestamp,
    sessions::{self, Activity, Backend, Details, LocalLaunchers, OutcomeKind, Session, Store},
    state_directory, time_text,
};
use rune::{Cursor, El, Latest, ScrollSpec, Tx, Waker, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, Kit};

use crate::{app::Window, problems::Room, theme};

#[derive(Default)]
pub struct SessionDialog {
    pub open: Option<Open>,
    reads: Latest<Result<Loaded>>,
    sample: Vec<Details>,
    refresh_at: Option<f64>,
}

pub struct Open {
    run: i64,
    count: u32,
    at: Timestamp,
    routine: String,
    store: Option<RunStore>,
    selected: usize,
    loaded: Option<Loaded>,
    failed: bool,
}

struct Loaded {
    sessions: Vec<Session>,
    details: Option<Details>,
}

impl SessionDialog {
    pub fn set_waker(&self, wake: Waker) {
        self.reads.set_waker(wake);
    }

    pub fn open(
        &mut self,
        run: i64,
        count: u32,
        at: Timestamp,
        routine: String,
        store: Option<RunStore>,
    ) {
        self.close();
        self.open = Some(Open {
            run,
            count,
            at,
            routine,
            store,
            selected: 0,
            loaded: None,
            failed: false,
        });
        self.request();
    }

    pub fn close(&mut self) {
        self.open = None;
        self.refresh_at = None;
        self.reads.cancel();
    }

    pub fn sample(&mut self, entries: Vec<Details>) {
        self.sample = entries;
    }

    fn pick(&mut self, selected: usize) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        if open.selected == selected {
            return;
        }
        open.selected = selected;
        if let Some(loaded) = &mut open.loaded {
            loaded.details = None;
        }
        self.request();
    }

    fn request(&mut self) {
        let Some(open) = &mut self.open else {
            return;
        };
        if !self.sample.is_empty() {
            open.loaded = Some(Loaded {
                sessions: self
                    .sample
                    .iter()
                    .map(|item| item.session.clone())
                    .collect(),
                details: self.sample.get(open.selected).cloned(),
            });
        } else if let Some(store) = open.store.clone() {
            self.reads.request(
                (store, open.run, open.selected),
                |(store, run, selected)| load(store, run, selected),
            );
        } else {
            open.failed = true;
        }
    }

    pub fn poll(&mut self, clock: f64, tx: &mut Tx) -> bool {
        let Some(open) = &mut self.open else {
            return false;
        };
        let answer = self.reads.take();
        let changed = answer.is_some();
        if let Some(answer) = answer {
            open.failed = answer.is_err();
            if let Ok(loaded) = answer {
                open.loaded = Some(loaded);
            }
            self.refresh_at = Some(clock + 5.0);
        }
        if let Some(when) = self.refresh_at {
            if clock >= when {
                self.refresh_at = None;
                self.request();
            } else {
                tx.wake_at(when);
            }
        }
        changed
    }
}

fn load(store: RunStore, run: i64, selected: usize) -> Result<Loaded> {
    let dir = match store {
        RunStore::Default => None,
        RunStore::Directory(path) => Some(path),
    };
    let root = state_directory::existing(dir)?.ok_or(Error::SessionNotFound)?;
    let store = Store::existing(&root)?.ok_or(Error::SessionNotFound)?;
    let sessions = store.for_run(run)?;
    let details = sessions
        .get(selected)
        .map(|session| {
            sessions::details(
                &store,
                &LocalLaunchers {
                    state_dir: root.path(),
                },
                &session.id,
            )
        })
        .transpose()?;
    Ok(Loaded { sessions, details })
}

fn title(count: u32) -> String {
    if count == 1 {
        "Session spawned".into()
    } else {
        format!("{count} sessions spawned")
    }
}

pub fn activity_button(key: &str, count: u32, kit: &Kit) -> El {
    let key = key.to_owned();
    let label = title(count);
    row()
        .self_start()
        .cursor(Cursor::Pointer)
        .hover(kit.c.hover)
        .press(kit.c.pressed)
        .label(label.clone())
        .key("session-activity")
        .on_press(move |window: &mut Window| window.show_sessions(&key))
        .child(
            text(format!("{label} ↗"))
                .font(theme::strong())
                .color(kit.c.info.fg)
                .wrap(),
        )
}

pub fn dialog(open: &Open, now: Timestamp, room: Room, kit: &Kit, layers: &mut Vec<El>) {
    let mut body = col().w_full().px(16.0).pb(16.0).gap(12.0);
    if let Some(loaded) = &open.loaded {
        if loaded.sessions.len() > 1 {
            body = body
                .child(picker(open.selected, &loaded.sessions, kit))
                .child(el().w_full().h(1.0).fill(kit.c.border_base));
        }
        if let Some(details) = &loaded.details {
            body = body.child(session(details, now, kit));
        } else if !open.failed {
            let message = if loaded.sessions.is_empty() {
                "No sessions available."
            } else {
                "Reading session…"
            };
            body = body.child(quiet(message, kit));
        }
    } else if !open.failed {
        body = body.child(quiet("Reading sessions…", kit));
    }
    if open.failed {
        body = body.child(quiet(
            "Couldn’t read session details. Rhythm will retry.",
            kit,
        ));
    }
    let viewport = col()
        .w_full()
        .max_h((room.window - 32.0 - room.header).clamp(96.0, 370.0))
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .key("session-details-scroll")
        .child(body);
    Dialog::new(
        "sessions",
        if open.count == 1 {
            "Session spawned"
        } else {
            "Sessions spawned"
        },
    )
    .description(format!(
        "{} · {} · Run #{}",
        open.routine,
        time_text::ago(open.at, now),
        open.run
    ))
    .width(520.0)
    .fit()
    .open(true)
    .on_close(|window: &mut Window| window.sessions.close())
    .child(kit.scroll_bar(viewport))
    .build(kit, layers);
}

fn picker(selected: usize, sessions: &[Session], kit: &Kit) -> El {
    if sessions.len() <= 3 {
        let mut controls = row().w_full().gap(6.0);
        for (index, session) in sessions.iter().enumerate() {
            let mut short: String = session.title.chars().take(20).collect();
            if short.len() < session.title.len() {
                short.push('…');
            }
            controls = controls.child(
                button(
                    short,
                    if selected == index {
                        ButtonVariant::Outline
                    } else {
                        ButtonVariant::Ghost
                    },
                )
                .help(session.title.clone())
                .on_press(move |window: &mut Window| window.sessions.pick(index))
                .build(kit)
                .key(format!("session-tab-{index}")),
            );
        }
        return controls;
    }
    row()
        .center_y()
        .gap(8.0)
        .child(
            button("Previous", ButtonVariant::Outline)
                .disabled(selected == 0)
                .on_press(move |window: &mut Window| {
                    window.sessions.pick(selected.saturating_sub(1))
                })
                .build(kit)
                .key("session-previous"),
        )
        .child(quiet(
            format!("{} of {}", selected + 1, sessions.len()),
            kit,
        ))
        .child(
            button("Next", ButtonVariant::Outline)
                .disabled(selected + 1 == sessions.len())
                .on_press(move |window: &mut Window| window.sessions.pick(selected + 1))
                .build(kit)
                .key("session-next"),
        )
}

fn session(details: &Details, now: Timestamp, kit: &Kit) -> El {
    let session = &details.session;
    let reported = session.outcome.as_ref().map(|outcome| outcome.kind);
    let reported = reported.filter(|kind| {
        *kind != OutcomeKind::Waiting
            || matches!(
                details.activity,
                Activity::Waiting | Activity::Idle | Activity::Unknown
            )
    });
    let (badge, fill, ink) = match reported {
        Some(OutcomeKind::Done) => ("Reported", kit.c.success.bg, kit.c.success.fg),
        Some(OutcomeKind::Failed) => ("Failed", kit.c.danger.bg, kit.c.danger.fg),
        Some(OutcomeKind::Waiting) => ("Waiting for input", kit.c.warning.bg, kit.c.warning.fg),
        None => match details.activity {
            Activity::Running => ("Running", kit.c.info.bg, kit.c.info.fg),
            Activity::Waiting => ("Waiting for input", kit.c.warning.bg, kit.c.warning.fg),
            Activity::Ended | Activity::Missing if session.outcome.is_some() => {
                ("Ended", kit.c.layer_01, kit.c.text_muted)
            }
            Activity::Ended | Activity::Missing | Activity::Idle => {
                ("Ended without a report", kit.c.layer_01, kit.c.text_muted)
            }
            Activity::Unknown => ("Status unavailable", kit.c.layer_01, kit.c.text_muted),
        },
    };
    let badge = row()
        .self_start()
        .px(6.0)
        .py(2.0)
        .radius(4.0)
        .fill(fill)
        .child(text(badge).font(theme::secondary()).color(ink));
    let provider = match session.backend {
        Backend::Claude => "Claude Code",
        Backend::T3 => "T3 Code",
    };
    let mut facts = format!(
        "{provider} · Started {}",
        time_text::ago(session.created_at, now)
    );
    if let Some(outcome) = &session.outcome {
        facts.push_str(&format!(
            " · Outcome reported {}",
            time_text::ago(outcome.reported_at, now)
        ));
    }
    let summary = session
        .outcome
        .as_ref()
        .map_or("The agent hasn’t reported an outcome yet.", |outcome| {
            &outcome.summary
        });
    let mut content = col()
        .w_full()
        .gap(6.0)
        .child(
            row()
                .center_y()
                .gap(8.0)
                .w_full()
                .child(
                    text(session.title.clone())
                        .font(theme::strong())
                        .color(kit.c.text_base)
                        .wrap(),
                )
                .child(badge),
        )
        .child(quiet(facts, kit))
        .child(
            text(summary)
                .font(theme::body())
                .color(kit.c.text_base)
                .wrap()
                .selectable(true)
                .my(4.0),
        );
    let mut actions = Vec::new();
    if let Some(launch) = &session.launch {
        if session.backend == Backend::Claude {
            actions.push(copy(
                "Copy attach command",
                format!("claude attach {}", launch.provider_id),
                "session-open",
                kit,
            ));
        } else if let Some(url) = launch.link.as_deref().filter(|url| sessions::web_link(url)) {
            actions.push(open_link(
                "Open thread",
                url,
                "session-open",
                ButtonVariant::Outline,
                kit,
            ));
        } else {
            let link = launch
                .link
                .clone()
                .unwrap_or_else(|| launch.provider_id.clone());
            let label = if launch.link.is_some() {
                "Copy thread link"
            } else {
                "Copy thread ID"
            };
            actions.push(copy(label, link, "session-open", kit));
        }
    }
    if let Some(outcome) = &session.outcome {
        for (index, url) in outcome.pull_requests.iter().enumerate() {
            if !sessions::web_link(url) {
                continue;
            }
            let label = url
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .filter(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
                .map_or_else(|| "Open PR ↗".to_owned(), |num| format!("Open PR #{num} ↗"));
            actions.push(open_link(
                &label,
                url,
                &format!("session-pr-{index}"),
                ButtonVariant::Submit,
                kit,
            ));
        }
    }
    for pair in actions.chunks(2) {
        content = content.child(row().gap(8.0).children(pair.iter().cloned()));
    }
    content
}

fn button(label: impl Into<String>, variant: ButtonVariant) -> Button {
    Button::new(label)
        .variant(variant)
        .control_size(ControlSize::Small)
}

fn open_link(label: &str, url: &str, key: &str, variant: ButtonVariant, kit: &Kit) -> El {
    let url = url.to_owned();
    button(label, variant)
        .help(url.clone())
        .on_press_tx(move |_: &mut Window, tx: &mut Tx| {
            if tx.open_url(&url).is_err() {
                tx.copy(url.clone());
            }
        })
        .build(kit)
        .self_start()
        .key(key)
}

fn copy(label: &str, command: String, key: &str, kit: &Kit) -> El {
    button(label, ButtonVariant::Outline)
        .on_press_tx(move |_: &mut Window, tx: &mut Tx| {
            tx.copy(command.clone());
        })
        .build(kit)
        .self_start()
        .key(key)
}

fn quiet(value: impl Into<String>, kit: &Kit) -> El {
    text(value.into())
        .font(theme::secondary())
        .color(kit.c.text_muted)
        .wrap()
}
