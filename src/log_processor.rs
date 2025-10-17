use crate::shell_config::{END_COMMAND_NAME, START_COMMAND_NAME};
use regex::Regex;
use std::sync::OnceLock;

static ANSI_REGEX: OnceLock<Regex> = OnceLock::new();
static COMMAND_MARKER_REGEX: OnceLock<Regex> = OnceLock::new();

fn get_ansi_regex() -> &'static Regex {
    ANSI_REGEX.get_or_init(|| {
        Regex::new(
            r"(?x)
            \x1b\[                    # ESC[ - Control Sequence Introducer
            (?:
                (?:[0-9]+(?:;[0-9]+)*)  # Numeric parameters separated by semicolons
                ?                       # Parameters are optional
                [a-zA-Z]                # Final letter command
            )
            |
            \x1b\]                    # ESC] - Operating System Command
            (?:
                [^\x07\x1b]*          # Any characters except BEL or ESC
                (?:\x07|\x1b\\)       # Terminated by BEL or ESC\
            )
            |
            \x1b                      # Single ESC followed by
            [PX^_]                    # DCS, SOS, PM, APC introducers
            .*?                       # Any content (non-greedy)
            \x1b\\                    # ST (String Terminator)
            |
            \x1b                      # ESC followed by any single char
            [^PX^_\[]                 # Except the multi-char sequence starters
            |
            \x08                      # Backspace character
            |
            \x0f                      # Shift In (SI)
            |
            \x0e                      # Shift Out (SO)
            ",
        )
        .expect("Failed to compile ANSI escape code regex")
    })
}

fn get_command_marker_regex() -> &'static Regex {
    COMMAND_MARKER_REGEX.get_or_init(|| {
        let pattern = format!(
            r"(?m)^{}\s.*$\n?|(?m)^{}\s.*$\n?",
            regex::escape(START_COMMAND_NAME),
            regex::escape(END_COMMAND_NAME)
        );
        Regex::new(&pattern).expect("Failed to compile command marker regex")
    })
}

pub fn truncate_after_end_command(input: &[u8]) -> Vec<u8> {
    let input_str = String::from_utf8_lossy(input);

    // Find the first occurrence of renlog-end-command and truncate everything after it
    if let Some(pos) = input_str.find(END_COMMAND_NAME) {
        // Find the end of the line containing the end command
        if let Some(newline_pos) = input_str[pos..].find('\n') {
            let truncate_pos = pos + newline_pos + 1; // Include the newline
            input_str[..truncate_pos].as_bytes().to_vec()
        } else {
            // No newline after end command, truncate at the end command
            input_str[..pos].as_bytes().to_vec()
        }
    } else {
        // No end command found, return original input
        input.to_vec()
    }
}

fn strip_ansi_codes_and_command_markers(input: &[u8]) -> Vec<u8> {
    let input_str = String::from_utf8_lossy(input);

    let ansi_regex = get_ansi_regex();
    let ansi_cleaned = ansi_regex.replace_all(&input_str, "");

    let command_regex = get_command_marker_regex();
    let fully_cleaned = command_regex.replace_all(&ansi_cleaned, "");

    fully_cleaned.into_owned().into_bytes()
}

// TODO: Make faster. Converting to String and back is inefficient.
fn replace_crlf_with_lf(input: Vec<u8>) -> Vec<u8> {
    let input_str = String::from_utf8_lossy(&input);
    let normalized = input_str.replace("\r\n", "\n");
    normalized.into_bytes()
}

pub fn process_command_log_content(input: &[u8]) -> Vec<u8> {
    let truncated = truncate_after_end_command(input);
    let striped = strip_ansi_codes_and_command_markers(&truncated);
    replace_crlf_with_lf(striped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_and_command_markers_combined() {
        let input = b"\x1b[31mRed text\x1b[0m\nrenlog-start-command echo test\nNormal output\nrenlog-end-command 0\n\x1b[32mGreen text\x1b[0m";
        let expected = b"Red text\nNormal output\nGreen text";
        assert_eq!(strip_ansi_codes_and_command_markers(input), expected);
    }

    #[test]
    fn test_preserve_normal_text_with_command_filtering() {
        let input = b"Normal text without any markers or escape codes";
        let expected = b"Normal text without any markers or escape codes";
        assert_eq!(strip_ansi_codes_and_command_markers(input), expected);
    }

    #[test]
    fn test_truncate_after_end_command_with_extra_content() {
        let input =
            b"Command output\nrenlog-end-command 0\n[/tmp/renlog/file.log]\nExtra unwanted content";
        let expected = b"Command output\nrenlog-end-command 0\n";
        assert_eq!(truncate_after_end_command(input), expected);
    }

    #[test]
    fn test_truncate_after_end_command_no_newline() {
        let input = b"Command output\nrenlog-end-command 0[/tmp/renlog/file.log]";
        let expected = b"Command output\n";
        assert_eq!(truncate_after_end_command(input), expected);
    }

    #[test]
    fn test_truncate_after_end_command_not_found() {
        let input = b"Normal command output\nNo end marker here";
        let expected = b"Normal command output\nNo end marker here";
        assert_eq!(truncate_after_end_command(input), expected);
    }

    #[test]
    fn test_truncate_after_end_command_multiple_occurrences() {
        let input = b"Content\nrenlog-end-command 0\nUnwanted\nrenlog-end-command 1\nMore unwanted";
        let expected = b"Content\nrenlog-end-command 0\n";
        assert_eq!(truncate_after_end_command(input), expected);
    }

    #[test]
    fn test_process_command_log_content_complete() {
        let input = b"\x1b[31mRed text\x1b[0m\nrenlog-start-command echo test\nActual output\nrenlog-end-command 0\n[/tmp/renlog/unwanted.log]\nExtra content";
        let expected = b"Red text\nActual output\n";
        assert_eq!(process_command_log_content(input), expected);
    }

    #[test]
    fn test_process_command_log_content_real_scenario() {
        let input = b"Cargo.lock  CLAUDE.md  README.md  target/     test_sigwinch.sh\nCargo.toml  LICENSE    src/       test_logs/  zenlog/\nrenlog-end-command 0\n[/tmp/renlog/20250812_130850_236_cat-_tmp_renlog_20250812_130843_139_ls--F---color_auto.log.log]";
        let expected = b"Cargo.lock  CLAUDE.md  README.md  target/     test_sigwinch.sh\nCargo.toml  LICENSE    src/       test_logs/  zenlog/\n";
        assert_eq!(process_command_log_content(input), expected);
    }
}
