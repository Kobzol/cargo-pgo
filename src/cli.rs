use std::fmt::Display;

use colored::{ColoredString, Colorize};

/// Formats a path in a unified format to be printed in CLI.
pub fn cli_format_path<P: Display>(path: P) -> ColoredString {
    path.to_string().yellow()
}
