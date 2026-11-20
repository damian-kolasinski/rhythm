use rhythm::{
    jobs::Health,
    routines::{Ownership, Source},
    time_text,
};
use rune::{Cx, El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit, SegmentedControl};

use crate::{
    app::Window,
    routine::{Filter, Routine},
    theme,
};

pub fn window(state: &Window, cx: &mut Cx<Window>) -> El {
    let kit = theme::kit(cx.system_dark());
    let all = state
        .snapshot
        .as_ref()
        .map_or(&[][..], |inventory| inventory.rows.as_slice());
    let rows: Vec<_> = all
        .iter()
        .filter(|job| state.filter.includes(job.ownership))
        .collect();
    let count = rows
        .iter()
        .filter(|routine| routine.state.needs_attention())
        .count();
    let mut meta = row().gap(theme::GAP);
    if state.snapshot.is_some() {
        let summary = if state.filter == Filter::All {
            format!("{} on this Mac", rows.len())
        } else {
            format!("{} of {} on this Mac", rows.len(), all.len())
        };
        meta = meta.child(quiet(summary, &kit));
    }
    if count > 0 {
        let verb = if count == 1 { "needs" } else { "need" };
        meta = meta.child(
            text(format!("· {count} {verb} attention"))
                .font(theme::secondary())
                .color(kit.c.danger.fg),
        );
    }
    let heading = row()
        .w_full()
        .center_y()
        .child(
            col()
                .grow(1.0)
                .gap(4.0)
                .child(
                    text("Routines")
                        .font(theme::heading())
                        .color(kit.c.text_base),
                )
                .child(meta),
        )
        .child(
            Button::new("Refresh")
                .variant(ButtonVariant::Outline)
                .on_press(Window::refresh)
                .build(&kit)
                .key("refresh"),
        );
    let mut body = col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .padding(theme::INSET)
        .gap(12.0)
        .child(heading);
    if let Some(title) = state.updates.title() {
        let colors = if state.updates.error.is_some() {
            kit.c.warning
        } else {
            kit.c.info
        };
        let label = if state.updates.working {
            "Upgrading…"
        } else {
            "Upgrade and reopen"
        };
        body = body.child(
            row()
                .w_full()
                .padding(12.0)
                .gap(12.0)
                .center_y()
                .radius(theme::RADIUS)
                .fill(colors.bg)
                .child(
                    col()
                        .grow(1.0)
                        .child(text(title).font(theme::strong()).color(colors.fg))
                        .child(
                            text(state.updates.message())
                                .font(theme::secondary())
                                .color(colors.fg)
                                .wrap(),
                        ),
                )
                .child(
                    Button::new(label)
                        .variant(ButtonVariant::Submit)
                        .disabled(state.updates.working)
                        .on_press(Window::upgrade)
                        .build(&kit)
                        .key("upgrade"),
                ),
        );
    }
    if state.failed {
        let hint = if state.snapshot.is_some() {
            "Showing the last successful refresh."
        } else {
            "Refresh to try again."
        };
        body = body.child(
            col()
                .w_full()
                .padding(12.0)
                .radius(theme::RADIUS)
                .fill(kit.c.warning.bg)
                .child(
                    text("Couldn’t refresh routines")
                        .font(theme::strong())
                        .color(kit.c.warning.fg),
                )
                .child(text(hint).font(theme::secondary()).color(kit.c.warning.fg)),
        );
    }
    if let Some(inventory) = &state.snapshot {
        if !inventory.incomplete.is_empty() {
            let sources = inventory
                .incomplete
                .iter()
                .map(|source| source_name(*source))
                .collect::<Vec<_>>()
                .join(", ");
            body = body.child(quiet(format!("Some sources unavailable · {sources}"), &kit).wrap());
        }
        let selected = Filter::OPTIONS
            .iter()
            .position(|filter| *filter == state.filter);
        body = body.child(
            SegmentedControl::new(&Filter::OPTIONS.map(Filter::label), selected)
                .label("Routine ownership")
                .width(Some(234.0))
                .on_pick(|window: &mut Window, index, _| window.filter = Filter::OPTIONS[index])
                .build(&kit)
                .key("ownership-filter"),
        );
    }
    let content = if !rows.is_empty() {
        table(state, &rows, &kit)
    } else {
        let (title, help) = match (state.snapshot.is_some(), state.failed, state.filter) {
            (true, _, Filter::All) => ("No routines found", ""),
            (true, _, Filter::Rhythm) => (
                "No Rhythm-managed routines on this Mac",
                "Create one with rhythm add or record an existing job with rhythm adopt.",
            ),
            (true, _, Filter::External) => ("No external periodic jobs found", ""),
            (false, true, _) => ("Routines unavailable", ""),
            (false, false, _) => ("Reading routines…", ""),
        };
        col()
            .w_full()
            .grow(1.0)
            .center()
            .gap(4.0)
            .child(text(title).font(theme::strong()).color(kit.c.text_base))
            .child(quiet(help, &kit))
    };
    let freshness = state
        .freshness
        .map(|at| format!("Routines refreshed {}", time_text::ago(at, state.now)))
        .unwrap_or_default();
    col()
        .w_full()
        .h_full()
        .fill(kit.c.page)
        .child(body.child(content))
        .child(rule(&kit))
        .child(
            row()
                .w_full()
                .h(36.0)
                .px(theme::INSET)
                .center_y()
                .fill(kit.c.layer_01)
                .child(el().grow(1.0).child(quiet(
                    format!("Rhythm {}", state.updates.displayed_version()),
                    &kit,
                )))
                .child(quiet(freshness, &kit)),
        )
}

fn table(state: &Window, rows: &[&Routine], kit: &Kit) -> El {
    let headings =
        ["Routine", "State", "Trigger", "Latest activity"].map(|title| quiet(title, kit));
    let mut list = col().w_full();
    for job in rows {
        let (activity, timing) = job.activity.text(state.now);
        let mut latest = col().child(
            text(activity)
                .font(theme::body())
                .color(kit.c.text_base)
                .wrap(),
        );
        if let Some(timing) = timing {
            latest = latest.child(quiet(timing, kit));
        }
        let name = col()
            .gap(1.0)
            .child(
                text(job.name.clone())
                    .font(theme::strong())
                    .color(kit.c.text_base)
                    .wrap(),
            )
            .child(
                text(job.label.clone())
                    .font(theme::mono())
                    .color(kit.c.text_muted)
                    .truncate_middle()
                    .selectable(true),
            )
            .child(ownership_badge(job.ownership, kit));
        list = list
            .child(
                columns([
                    name,
                    pill(job.state, kit),
                    text(job.trigger.clone())
                        .font(theme::body())
                        .color(kit.c.text_base)
                        .wrap(),
                    latest,
                ])
                .min_h(55.0)
                .py(5.0)
                .key(job.key.clone()),
            )
            .child(rule(kit));
    }
    let viewport = col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .key(format!("routine-list-{}", state.filter.label()))
        .child(list);
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(columns(headings).h(26.0))
        .child(rule(kit))
        .child(kit.scroll_bar(viewport))
}

fn columns(cells: [El; 4]) -> El {
    let mut line = row().w_full().center_y();
    for (cell, width) in cells.into_iter().zip([0.34, 0.16, 0.23, 0.27]) {
        line = line.child(col().w_frac(width).pr(8.0).child(cell));
    }
    line
}

fn pill(state: Health, kit: &Kit) -> El {
    let (label, fill, ink) = match state {
        Health::Invalid => ("Invalid", kit.c.danger.bg, kit.c.danger.fg),
        Health::Failing => ("Failing", kit.c.danger.bg, kit.c.danger.fg),
        Health::Down => ("Down", kit.c.warning.bg, kit.c.warning.fg),
        Health::Running => ("Running", kit.c.info.bg, kit.c.info.fg),
        Health::Ok => ("OK", kit.c.success.bg, kit.c.success.fg),
        Health::Idle => ("Idle", kit.c.layer_01, kit.c.text_muted),
        Health::Unloaded => ("Unloaded", kit.c.layer_01, kit.c.text_muted),
        Health::Unknown => ("Unknown", kit.c.layer_01, kit.c.text_muted),
    };
    row()
        .self_start()
        .center_y()
        .gap(5.0)
        .px(8.0)
        .h(22.0)
        .radius(theme::RADIUS)
        .fill(fill)
        .child(el().size(5.0, 5.0).radius(2.5).fill(ink))
        .child(text(label).font(theme::secondary()).color(ink))
}

fn ownership_badge(ownership: Ownership, kit: &Kit) -> El {
    let (label, fill, ink) = match ownership {
        Ownership::Created => ("Created by Rhythm", kit.c.info.bg, kit.c.info.fg),
        Ownership::Adopted => ("Adopted by Rhythm", kit.c.info.bg, kit.c.info.fg),
        Ownership::External => ("External", kit.c.layer_01, kit.c.text_muted),
    };
    row()
        .self_start()
        .center_y()
        .px(6.0)
        .h(17.0)
        .radius(4.0)
        .fill(fill)
        .child(text(label).font(theme::secondary()).color(ink))
}

fn source_name(source: Source) -> &'static str {
    match source {
        Source::UserLaunchAgents => "user LaunchAgents",
        Source::SystemLaunchdDefinitions => "system launchd definitions",
        Source::LaunchdStatus => "launchd status",
        Source::UserCrontab => "user crontab",
        Source::SystemCrontabs => "system crontabs",
    }
}

fn quiet(value: impl Into<String>, kit: &Kit) -> El {
    text(value.into())
        .font(theme::secondary())
        .color(kit.c.text_muted)
}

fn rule(kit: &Kit) -> El {
    el().h(1.0).w_full().fill(kit.c.border_base)
}
