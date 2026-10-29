//! The guide agents follow, embedded so `rhythm skill show` describes the
//! installed version. Its references are the repository's own docs.

/// The skill's folder name, which is also its `name`.
pub const NAME: &str = "rhythm";

const SKILL: &str = include_str!("../skill/SKILL.md");

/// Each reference `rhythm skill show NAME` prints, by name.
pub const REFERENCES: &[(&str, &str)] = &[
    ("status", include_str!("../docs/status.md")),
    ("runs", include_str!("../docs/runs.md")),
    ("authoring", include_str!("../docs/authoring.md")),
    ("attention", include_str!("../docs/attention.md")),
    ("agents", include_str!("../docs/agents.md")),
];

/// The guide, or the reference `name`; `None` for a name no reference has.
/// Links between documents become the command that prints them.
pub fn show(name: Option<&str>) -> Option<String> {
    let text = match name {
        None => body(),
        Some(name) => REFERENCES.iter().find(|(n, _)| *n == name)?.1,
    };
    Some(commands_for_links(text))
}

/// The guide without its frontmatter.
pub fn body() -> &'static str {
    frontmatter().map_or(SKILL, |(_, body)| body.trim_start())
}

/// The `description` in the frontmatter, which tells agents when to load the skill.
pub fn description() -> &'static str {
    frontmatter()
        .and_then(|(head, _)| head.lines().find_map(|l| l.strip_prefix("description: ")))
        .unwrap_or_default()
}

fn frontmatter() -> Option<(&'static str, &'static str)> {
    SKILL.strip_prefix("---\n")?.split_once("\n---\n")
}

/// `[text](runs.md)` becomes ``text (`rhythm skill show runs`)``.
fn commands_for_links(text: &str) -> String {
    let mut out = text.to_owned();
    for (name, _) in REFERENCES {
        let target = format!("]({name}.md)");
        while let Some(end) = out.find(&target) {
            let Some(start) = out[..end].rfind('[') else {
                break;
            };
            let label = out[start + 1..end].to_owned();
            out.replace_range(
                start..end + target.len(),
                &format!("{label} (`rhythm skill show {name}`)"),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_names_itself_and_says_when_to_load() {
        assert!(SKILL.starts_with(&format!("---\nname: {NAME}\n")));
        assert!(description().starts_with("Find, create, and debug background jobs"));
        assert!(body().starts_with("# Rhythm"));
    }

    #[test]
    fn every_reference_the_guide_names_exists() {
        for (name, _) in REFERENCES {
            assert!(body().contains(&format!("- `{name}`")), "{name}");
            assert!(show(Some(name)).is_some());
        }
        assert_eq!(show(Some("distribution")), None);
    }

    #[test]
    fn links_become_commands() {
        assert_eq!(
            commands_for_links("See [recorded runs](runs.md) and [x](other.md)."),
            "See recorded runs (`rhythm skill show runs`) and [x](other.md)."
        );
    }
}
