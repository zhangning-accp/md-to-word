pub mod convert;
mod math;

/// Escape user text for XML 1.0, including invalid control characters.
pub(crate) fn xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if c >= '\u{20}' && c != '\u{fffe}' && c != '\u{ffff}' => out.push(c),
            _ => out.push('\u{fffd}'),
        }
    }
    out
}
