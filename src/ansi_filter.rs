use regex::Regex;
use std::sync::OnceLock;

static ANSI_REGEX: OnceLock<Regex> = OnceLock::new();

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

pub fn strip_ansi_codes(input: &[u8]) -> Vec<u8> {
    let input_str = String::from_utf8_lossy(input);

    let regex = get_ansi_regex();
    let cleaned = regex.replace_all(&input_str, "");

    cleaned.into_owned().into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_color_codes() {
        let input = b"\x1b[31mRed text\x1b[0m Normal text";
        let expected = b"Red text Normal text";
        assert_eq!(strip_ansi_codes(input), expected);
    }

    #[test]
    fn test_strip_cursor_movement() {
        let input = b"Text\x1b[2AMove up\x1b[3BMove down";
        let expected = b"TextMove upMove down";
        assert_eq!(strip_ansi_codes(input), expected);
    }

    #[test]
    fn test_strip_complex_sequences() {
        let input = b"\x1b[1;31mBold Red\x1b[0m\x1b[2J\x1b[HClear and home";
        let expected = b"Bold RedClear and home";
        assert_eq!(strip_ansi_codes(input), expected);
    }

    #[test]
    fn test_preserve_normal_text() {
        let input = b"Normal text without escape codes";
        let expected = b"Normal text without escape codes";
        assert_eq!(strip_ansi_codes(input), expected);
    }

    #[test]
    fn test_strip_osc_sequences() {
        let input = b"Text\x1b]0;Window Title\x07More text";
        let expected = b"TextMore text";
        assert_eq!(strip_ansi_codes(input), expected);
    }

    #[test]
    fn test_strip_backspace() {
        let input = b"Text\x08Backspace";
        let expected = b"TextBackspace";
        assert_eq!(strip_ansi_codes(input), expected);
    }
}
