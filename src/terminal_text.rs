//! Text from files and other machines, made safe to print in a terminal.

/// Replaces terminal controls and invisible reordering characters, which could
/// otherwise rewrite the screen or disguise a label.
pub fn clean(text: &str) -> String {
    text.chars()
        .map(|c| {
            let invisible = matches!(
                c,
                '\u{200B}'..='\u{200F}'
                    | '\u{202A}'..='\u{202E}'
                    | '\u{2060}'..='\u{2069}'
                    | '\u{061C}'
                    | '\u{FEFF}'
            );
            if c.is_control() || invisible { '?' } else { c }
        })
        .collect()
}
