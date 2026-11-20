//! The dialog listing a routine's open problems: what each one is, how long it
//! has been open, who handles it, and where to read more.
use rhythm::{findings::Finding, runs::Timestamp, time_text};
use rune::{El, ScrollSpec, Tx, col, el, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, Kit};

use crate::{app::Window, routine::Routine, theme};

/// The space Kit keeps around a dialog.
const MARGIN: f32 = 16.0;

/// The room the dialog has: the window's height and the height of Kit's
/// header above the list, which the dialog never outgrows; the list scrolls
/// instead.
pub struct Room {
    pub window: f32,
    pub header: f32,
}

impl Room {
    /// Before the first layout, the default window and Kit's header with a
    /// title and a description.
    pub const DEFAULT: Self = Self {
        window: 462.0,
        header: 76.0,
    };
}

pub fn dialog(routine: &Routine, now: Timestamp, room: Room, kit: &Kit, layer: &mut Vec<El>) {
    let mut list = col().w_full().px(16.0).pb(16.0);
    for (index, finding) in routine.problems.iter().enumerate() {
        if index > 0 {
            list = list.child(el().h(1.0).w_full().my(12.0).fill(kit.c.border_base));
        }
        list = list.child(problem(finding, now, kit).key(finding.problem.key.as_str()));
    }
    if routine.unlisted > 0 {
        list = list.child(
            text(format!(
                "The job listed {} more beyond the first 100; they are not kept.",
                routine.unlisted
            ))
            .font(theme::secondary())
            .color(kit.c.text_muted)
            .wrap()
            .mt(16.0)
            .key("unlisted"),
        );
    }
    let room = (room.window - 2.0 * MARGIN - room.header).clamp(96.0, 360.0);
    let viewport = col()
        .w_full()
        .max_h(room)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .key("problem-list")
        .child(list);
    Dialog::new("problems", routine.name.clone())
        .description(format!("{} · {}", routine.label, routine.trigger))
        .width(520.0)
        .fit()
        .open(true)
        .on_close(Window::close_problems)
        .child(kit.scroll_bar(viewport))
        .build(kit, layer);
}

fn problem(finding: &Finding, now: Timestamp, kit: &Kit) -> El {
    let problem = &finding.problem;
    let mut facts = vec![format!(
        "Open for {}",
        time_text::duration(now.0 - finding.first_seen.0)
    )];
    if let Some(who) = &problem.handled_by {
        facts.push(format!("handled by {who}"));
    }
    let mut item = col()
        .w_full()
        .gap(4.0)
        .child(
            text(problem.title.to_string())
                .font(theme::strong())
                .color(kit.c.text_base)
                .wrap()
                .selectable(true),
        )
        .child(
            text(facts.join(" · "))
                .font(theme::secondary())
                .color(kit.c.text_muted)
                .wrap(),
        );
    if let Some(detail) = &problem.detail {
        item = item.child(
            text(detail.to_string())
                .font(theme::body())
                .color(kit.c.text_base)
                .wrap()
                .selectable(true),
        );
    }
    match problem.link.as_ref().map(|link| Link::of(link.as_str())) {
        Some(Link::Web(url)) => {
            let url = url.to_owned();
            item.child(
                Button::new("Open link")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Small)
                    .help(url.clone())
                    .on_press_tx(move |_: &mut Window, tx: &mut Tx| {
                        // A link the system refuses opens nothing; it is shown in the help.
                        let _ = tx.open_url(&url);
                    })
                    .build(kit)
                    .self_start()
                    .mt(4.0)
                    .key("open-link"),
            )
        }
        Some(Link::Text(link)) => item.child(
            text(link.to_owned())
                .font(theme::mono())
                .color(kit.c.text_muted)
                .wrap()
                .selectable(true),
        ),
        None => item,
    }
}

/// Where a problem says to read more: only a plain web address opens in the
/// browser, anything else is text to copy.
#[derive(Debug, PartialEq, Eq)]
enum Link<'a> {
    Web(&'a str),
    Text(&'a str),
}

impl<'a> Link<'a> {
    fn of(link: &'a str) -> Self {
        if Self::web(link) {
            Self::Web(link)
        } else {
            Self::Text(link)
        }
    }

    /// An `http` or `https` address the system opens as it reads: printable
    /// ASCII without the characters links must escape, a host with an
    /// optional port and no user name, which could disguise the host.
    fn web(link: &str) -> bool {
        let Some((scheme, rest)) = link.split_once("://") else {
            return false;
        };
        let scheme = scheme.to_ascii_lowercase();
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let (host, port) = authority.split_once(':').unwrap_or((authority, ""));
        matches!(scheme.as_str(), "http" | "https")
            && link.bytes().all(|b| b.is_ascii_graphic())
            && !link.contains(['\\', '"', '<', '>', '`', '^', '{', '|', '}', '[', ']'])
            && Self::escaped(link)
            && !host.is_empty()
            && !authority.contains('@')
            && port.bytes().all(|b| b.is_ascii_digit())
    }

    /// Every `%` starts an escape of two hex digits other than `%00`.
    fn escaped(link: &str) -> bool {
        link.split('%').skip(1).all(|escape| {
            let digits = escape.as_bytes().get(..2);
            digits.is_some_and(|d| d.iter().all(u8::is_ascii_hexdigit) && d != b"00")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Link;

    #[test]
    fn only_web_addresses_open_in_the_browser() {
        for web in [
            "https://example.com/pull/123",
            "http://example.com",
            "HTTPS://example.com/a?b=c#d",
            "https://example.com:8443/a%20b",
        ] {
            assert_eq!(Link::of(web), Link::Web(web));
        }
        for text in [
            "file:///etc/hosts",
            "javascript:alert(1)",
            "mailto:team@example.com",
            "example.com/pull/123",
            "https://",
            "https://example.com/a b",
            "https://example.com/q?x={id}",
            "https://?x",
            "https://:443/",
            "https://example.com@example.org/",
            "https://example.com:port/",
            "https://example.com/100%",
            "https://example.com/%zz",
            "https://example.com/%00",
            "https://exämple.com/",
            "see the example runbook",
            "ftp://example.com",
            "httpsx://example.com",
        ] {
            assert_eq!(Link::of(text), Link::Text(text));
        }
    }
}
