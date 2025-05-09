pub mod images;
pub mod issue;
pub mod shared_writer;
pub mod ui;

use std::io::Write;
use std::time::Duration;

use aws_smithy_types::{
    Document,
    Number as SmithyNumber,
};
use eyre::Result;

use super::ChatError;
use super::token_counter::TokenCounter;
use crate::util::system_info::in_cloudshell;

const GOV_REGIONS: &[&str] = &["us-gov-east-1", "us-gov-west-1"];

pub fn region_check(capability: &'static str) -> eyre::Result<()> {
    let Ok(region) = std::env::var("AWS_REGION") else {
        return Ok(());
    };

    if in_cloudshell() && GOV_REGIONS.contains(&region.as_str()) {
        eyre::bail!("AWS GovCloud ({region}) is not supported for {capability}.");
    }

    Ok(())
}

pub fn truncate_safe(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }

    let mut byte_count = 0;
    let mut char_indices = s.char_indices();

    for (byte_idx, _) in &mut char_indices {
        if byte_count + (byte_idx - byte_count) > max_bytes {
            break;
        }
        byte_count = byte_idx;
    }

    &s[..byte_count]
}

pub fn animate_output(output: &mut impl Write, bytes: &[u8]) -> Result<(), ChatError> {
    for b in bytes.chunks(12) {
        output.write_all(b)?;
        std::thread::sleep(Duration::from_millis(16));
    }
    Ok(())
}

/// Play the terminal bell notification sound
pub fn play_notification_bell(requires_confirmation: bool) {
    // Don't play bell for tools that don't require confirmation
    if !requires_confirmation {
        return;
    }

    // Check if we should play the bell based on terminal type
    if should_play_bell() {
        print!("\x07"); // ASCII bell character
        std::io::stdout().flush().unwrap();
    }
}

/// Determine if we should play the bell based on terminal type
fn should_play_bell() -> bool {
    // Get the TERM environment variable
    if let Ok(term) = std::env::var("TERM") {
        // List of terminals known to handle bell character well
        let bell_compatible_terms = [
            "xterm",
            "xterm-256color",
            "screen",
            "screen-256color",
            "tmux",
            "tmux-256color",
            "rxvt",
            "rxvt-unicode",
            "linux",
            "konsole",
            "gnome",
            "gnome-256color",
            "alacritty",
            "iterm2",
        ];

        // Check if the current terminal is in the compatible list
        for compatible_term in bell_compatible_terms.iter() {
            if term.starts_with(compatible_term) {
                return true;
            }
        }

        // For other terminals, don't play the bell
        return false;
    }

    // If TERM is not set, default to not playing the bell
    false
}

/// This is a simple greedy algorithm that drops the largest files first
/// until the total size is below the limit
///
/// # Arguments
/// * `files` - A mutable reference to a vector of tuples: (filename, content). This file will be
///   sorted but the content will not be changed.
///
/// Returns the dropped files
pub fn drop_matched_context_files(files: &mut [(String, String)], limit: usize) -> Result<Vec<(String, String)>> {
    files.sort_by(|a, b| TokenCounter::count_tokens(&b.1).cmp(&TokenCounter::count_tokens(&a.1)));
    let mut total_size = 0;
    let mut dropped_files = Vec::new();

    for (filename, content) in files.iter() {
        let size = TokenCounter::count_tokens(content);
        if total_size + size > limit {
            dropped_files.push((filename.clone(), content.clone()));
        } else {
            total_size += size;
        }
    }
    Ok(dropped_files)
}

pub fn serde_value_to_document(value: serde_json::Value) -> Document {
    match value {
        serde_json::Value::Null => Document::Null,
        serde_json::Value::Bool(bool) => Document::Bool(bool),
        serde_json::Value::Number(number) => {
            if let Some(num) = number.as_u64() {
                Document::Number(SmithyNumber::PosInt(num))
            } else if number.as_i64().is_some_and(|n| n < 0) {
                Document::Number(SmithyNumber::NegInt(number.as_i64().unwrap()))
            } else {
                Document::Number(SmithyNumber::Float(number.as_f64().unwrap_or_default()))
            }
        },
        serde_json::Value::String(string) => Document::String(string),
        serde_json::Value::Array(vec) => {
            Document::Array(vec.clone().into_iter().map(serde_value_to_document).collect::<_>())
        },
        serde_json::Value::Object(map) => Document::Object(
            map.into_iter()
                .map(|(k, v)| (k, serde_value_to_document(v)))
                .collect::<_>(),
        ),
    }
}

pub fn document_to_serde_value(value: Document) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Document::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, document_to_serde_value(v)))
                .collect::<_>(),
        ),
        Document::Array(vec) => Value::Array(vec.clone().into_iter().map(document_to_serde_value).collect::<_>()),
        Document::Number(number) => {
            if let Ok(v) = TryInto::<u64>::try_into(number) {
                Value::Number(v.into())
            } else if let Ok(v) = TryInto::<i64>::try_into(number) {
                Value::Number(v.into())
            } else {
                Value::Number(
                    serde_json::Number::from_f64(number.to_f64_lossy())
                        .unwrap_or(serde_json::Number::from_f64(0.0).expect("converting from 0.0 will not fail")),
                )
            }
        },
        Document::String(s) => serde_json::Value::String(s),
        Document::Bool(b) => serde_json::Value::Bool(b),
        Document::Null => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_safe() {
        assert_eq!(truncate_safe("Hello World", 5), "Hello");
        assert_eq!(truncate_safe("Hello ", 5), "Hello");
        assert_eq!(truncate_safe("Hello World", 11), "Hello World");
        assert_eq!(truncate_safe("Hello World", 15), "Hello World");
    }

    #[test]
    fn test_drop_matched_context_files() {
        let mut files = vec![
            ("file1".to_string(), "This is a test file".to_string()),
            (
                "file3".to_string(),
                "Yet another test file that's has the largest context file".to_string(),
            ),
        ];
        let limit = 10;

        let dropped_files = drop_matched_context_files(&mut files, limit).unwrap();
        assert_eq!(dropped_files.len(), 1);
        assert_eq!(dropped_files[0].0, "file3");
        assert_eq!(files.len(), 2);

        for (filename, _) in dropped_files.iter() {
            files.retain(|(f, _)| f != filename);
        }
        assert_eq!(files.len(), 1);
    }
}
