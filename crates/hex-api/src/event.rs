//! Hex's event lines on a process's log (CLAUDE.md §10): `hex.<area> event=<name> key=value ...`, one line each. A
//! value that may hold a space, a quote, a backslash or a newline (a path, an error's words, what a wait follows) is
//! written as a JSON string (RFC 8259 §7), so nothing it says can split an event across lines or run into the next
//! key. kiln's metrics read these lines (`hex.wait event=ran_out`, ROOT and kiln's metrics owner, 2026-11-14).

/// `text` as a JSON string, quoted (RFC 8259 §7): a `"` and a `\` escaped by a `\`, and every control character,
/// U+0000 to U+001F, escaped as `\b`, `\f`, `\n`, `\r`, `\t` or `\u00XX`. Every other character is itself.
pub fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_add(2));
    out.push('"');
    push_json_contents(&mut out, text, usize::MAX);
    out.push('"');
    out
}

/// Appends `text` to `line` as a JSON string's contents, escaped as [`json_string`] escapes, a character at a time
/// while `line` stays within `bound` bytes; says how many of `text`'s bytes didn't fit, none cut within an escape or
/// a character.
pub fn push_json_contents(line: &mut String, text: &str, bound: usize) -> usize {
    for (at, c) in text.char_indices() {
        let before = line.len();
        match c {
            '"' => line.push_str("\\\""),
            '\\' => line.push_str("\\\\"),
            '\n' => line.push_str("\\n"),
            '\r' => line.push_str("\\r"),
            '\t' => line.push_str("\\t"),
            '\u{8}' => line.push_str("\\b"),
            '\u{c}' => line.push_str("\\f"),
            c if u32::from(c) < 0x20 => line.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => line.push(c),
        }
        if line.len() > bound {
            line.truncate(before);
            return text.len().saturating_sub(at);
        }
    }
    0
}

#[cfg(test)]
mod tests;
