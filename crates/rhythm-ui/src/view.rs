use rhythm::{jobs::Health, time_text};
use rune::{Cx, El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit};

use crate::{app::Window, routine::Routine, theme};

pub fn window(state: &Window, cx: &mut Cx<Window>) -> El {
    let kit = theme::kit(cx.system_dark());
    let rows = state.snapshot.as_deref().unwrap_or_default();
    let count = rows
        .iter()
        .filter(|routine| routine.state.needs_attention())
        .count();
    let mut meta = row().gap(theme::GAP);
    if state.snapshot.is_some() {
        meta = meta.child(quiet(format!("{} on this Mac", rows.len()), &kit));
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
    let content = if !rows.is_empty() {
        table(state, rows, &kit)
    } else {
        let (title, help) = match (state.snapshot.is_some(), state.failed) {
            (true, _) => (
                "No Rhythm-managed routines on this Mac",
                "Create one with rhythm add or record an existing job with rhythm adopt.",
            ),
            (false, true) => ("Routines unavailable", ""),
            (false, false) => ("Reading routines…", ""),
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
                .child(
                    el().grow(1.0)
                        .child(quiet(format!("Rhythm {}", env!("CARGO_PKG_VERSION")), &kit)),
                )
                .child(quiet(freshness, &kit)),
        )
}

fn table(state: &Window, rows: &[Routine], kit: &Kit) -> El {
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
            );
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
                .py(7.0)
                .key(job.label.clone()),
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
        .key("routine-list")
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

fn quiet(value: impl Into<String>, kit: &Kit) -> El {
    text(value.into())
        .font(theme::secondary())
        .color(kit.c.text_muted)
}

fn rule(kit: &Kit) -> El {
    el().h(1.0).w_full().fill(kit.c.border_base)
}
