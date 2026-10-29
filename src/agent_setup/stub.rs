//! The `SKILL.md` setup puts where agents load skills: the name and
//! description that tell an agent when to load it, the one rule that must not
//! wait for that, and a pointer to `rhythm skill show`, which prints the guide
//! of whichever Rhythm is installed. Upgrading Rhythm updates what agents read.
use crate::skill_guide::{self, NAME};

/// Bumped with any change to the text, so a folder shows which Rhythm wrote
/// it, and an older Rhythm never replaces a newer stub.
pub const VERSION: u32 = 1;
/// What marks a stub of any version.
const MARKER: &str = "\n  rhythm-stub: \"";

pub fn text() -> String {
    format!(
        "---\n\
         name: {NAME}\n\
         description: {description}\n\
         metadata:\n  rhythm-stub: \"{VERSION}\"\n\
         ---\n\
         <!-- Written by `rhythm skill setup`, which keeps it current; `rhythm skill remove` removes it. -->\n\
         \n\
         # Rhythm\n\
         \n\
         Never write launchd plists by hand for recurring work: `rhythm add` creates jobs that \
         record their runs, and `rhythm adopt` records an existing job's.\n\
         \n\
         Run `rhythm skill show` and follow the guide it prints. It comes from the installed \
         Rhythm, so it describes that version's commands; `rhythm skill show NAME` prints each \
         reference the guide names.\n",
        description = skill_guide::description(),
    )
}

/// The version of a stub some Rhythm wrote; `None` for any other skill.
pub fn version(skill_md: &str) -> Option<u32> {
    if !skill_md.starts_with(&format!("---\nname: {NAME}\n")) {
        return None;
    }
    let rest = &skill_md[skill_md.find(MARKER)? + MARKER.len()..];
    rest[..rest.find('"')?].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_its_own_stubs_only() {
        assert_eq!(version(&text()), Some(VERSION));
        assert_eq!(
            version(&text().replace("rhythm-stub: \"1\"", "rhythm-stub: \"7\"")),
            Some(7)
        );
        assert_eq!(
            version("---\nname: rhythm\ndescription: the operator's own\n---\n"),
            None
        );
        assert_eq!(
            version(&text().replace("name: rhythm", "name: other")),
            None
        );
    }

    #[test]
    fn the_stub_carries_the_guides_description() {
        assert!(text().contains(&format!("description: {}\n", skill_guide::description())));
    }
}
