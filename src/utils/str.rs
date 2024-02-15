use std::borrow::Cow;

/// Adds `s` to the end of `text` if `count` is one.
pub fn pluralize(text: &str, count: usize) -> Cow<str> {
    if count == 1 {
        text.into()
    } else {
        format!("{}s", text).into()
    }
}

/// Makes the first character of the string uppercase, if it already wasn't uppercase.
pub fn capitalize(text: &str) -> Cow<str> {
    let mut chars = text.chars();
    let first = chars.next();
    match first {
        Some(char) if char.is_uppercase() => text.into(),
        Some(char) => {
            let rest: String = chars.collect();
            format!("{}{}", char.to_uppercase(), rest).into()
        }
        None => text.into(),
    }
}
