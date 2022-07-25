use std::borrow::Cow;

/// Adds `s` to the end of `text` if `count` is one.
pub fn pluralize(text: &str, count: usize) -> Cow<str> {
    if count == 1 {
        text.into()
    } else {
        format!("{}s", text).into()
    }
}
