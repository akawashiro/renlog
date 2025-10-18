use crate::shell_config::{END_COMMAND_NAME, START_COMMAND_NAME};
use regex::Regex;
use std::sync::OnceLock;

static ANSI_REGEX: OnceLock<Regex> = OnceLock::new();
static COMMAND_MARKER_REGEX: OnceLock<Regex> = OnceLock::new();
static PASSWORD_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();

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

fn get_password_patterns() -> &'static Vec<Regex> {
    PASSWORD_PATTERNS.get_or_init(|| {
        vec![
            // Command line flags: -p password, --password=secret, --password secret
            Regex::new(r"(?i)(-p|--password|--pass)(=|\s+)(\S+)").unwrap(),
            // API keys and tokens: --api-key=value, --token=value, -k value
            Regex::new(r"(?i)(--api[-_]?key|--token|--auth[-_]?token|-k)(=|\s+)(\S+)").unwrap(),
            // Connection strings: mysql://user:pass@host, postgres://user:pass@host
            Regex::new(
                r"(?i)(mysql|postgresql|postgres|mongodb|redis|amqp|ftp|sftp)://([^:]+):([^@]+)@",
            )
            .unwrap(),
            // HTTP(S) URLs with credentials: https://user:pass@host
            Regex::new(r"(https?://)([^:]+):([^@]+)@").unwrap(),
            // Environment-style variables: PASSWORD=secret, API_KEY=secret
            Regex::new(
                r"(?i)(PASSWORD|PASSWD|PWD|API_KEY|SECRET|TOKEN|AUTH_TOKEN|ACCESS_TOKEN)=([\S]+)",
            )
            .unwrap(),
            // MySQL command variations: mysql -u user -pPassword
            Regex::new(r"(?i)mysql\s+.*-p(\S+)").unwrap(),
            // wget/curl with user credentials: --user=user:pass
            Regex::new(r"(?i)(--user|--ftp-user)(=|\s+)([^:\s]+):(\S+)").unwrap(),
        ]
    })
}

fn mask_passwords(input: &str) -> String {
    let patterns = get_password_patterns();
    let mut result = input.to_string();

    for (idx, pattern) in patterns.iter().enumerate() {
        result = pattern
            .replace_all(&result, |caps: &regex::Captures| {
                let full_match = caps.get(0).unwrap().as_str();

                // Different replacement strategies based on pattern type
                match idx {
                    0 | 1 => {
                        // Command line flags: -p password, --password=secret, --api-key=value
                        if let (Some(flag), Some(sep)) = (caps.get(1), caps.get(2)) {
                            format!("{}{}***", flag.as_str(), sep.as_str())
                        } else {
                            "***".to_string()
                        }
                    }
                    2 => {
                        // Connection strings: mysql://user:pass@host
                        if let (Some(scheme), Some(username)) = (caps.get(1), caps.get(2)) {
                            format!("{}://{}:***@", scheme.as_str(), username.as_str())
                        } else {
                            full_match.to_string()
                        }
                    }
                    3 => {
                        // HTTP(S) URLs: https://user:pass@host
                        if let (Some(protocol), Some(username)) = (caps.get(1), caps.get(2)) {
                            format!("{}{}:***@", protocol.as_str(), username.as_str())
                        } else {
                            full_match.to_string()
                        }
                    }
                    4 => {
                        // Environment variables: PASSWORD=secret
                        if let Some(key) = caps.get(1) {
                            format!("{}=***", key.as_str())
                        } else {
                            "***".to_string()
                        }
                    }
                    5 => {
                        // MySQL -p flag: mysql -pPassword
                        "mysql -p***".to_string()
                    }
                    6 => {
                        // wget/curl --user: --user=username:password
                        if let (Some(flag), Some(sep), Some(username)) =
                            (caps.get(1), caps.get(2), caps.get(3))
                        {
                            format!("{}{}{}:***", flag.as_str(), sep.as_str(), username.as_str())
                        } else {
                            "***".to_string()
                        }
                    }
                    _ => {
                        // Default fallback
                        "***".to_string()
                    }
                }
            })
            .to_string();
    }

    result
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

    // Mask passwords in the cleaned output
    let password_masked = mask_passwords(&fully_cleaned);

    password_masked.into_bytes()
}

// TODO: Make faster. Converting to String and back is inefficient.
fn replace_crlf_with_lf(input: Vec<u8>) -> Vec<u8> {
    let input_str = String::from_utf8_lossy(&input);
    let normalized = input_str.replace("\r\n", "\n");
    normalized.into_bytes()
}

pub fn mask_passwords_in_text(text: &str) -> String {
    mask_passwords(text)
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

    #[test]
    fn test_mask_passwords_command_line_flags() {
        // Test -p flag
        let input = "mysql -u root -psecret123 -h localhost";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("secret123"));
        assert!(result.contains("***"));

        // Test --password= flag
        let input = "psql --host=localhost --password=mypass --user=admin";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("mypass"));
        assert!(result.contains("***"));

        // Test --password with space
        let input = "someapp --password supersecret --debug";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("supersecret"));
        assert!(result.contains("***"));
    }

    #[test]
    fn test_mask_passwords_connection_strings() {
        // Test MySQL connection string
        let input = "mysql://admin:password123@localhost:3306/mydb";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("password123"));
        assert!(result.contains("***"));
        assert!(result.contains("admin"));
        assert!(result.contains("mysql://"));

        // Test PostgreSQL connection string
        let input = "postgresql://user:secret@host:5432/database";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("secret"));
        assert!(result.contains("***"));

        // Test MongoDB connection string
        let input = "mongodb://dbuser:dbpass@mongodb.example.com/mydatabase";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("dbpass"));
        assert!(result.contains("***"));
    }

    #[test]
    fn test_mask_passwords_http_urls() {
        // Test HTTPS URL with credentials
        let input = "curl https://user:pass123@api.example.com/endpoint";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("pass123"));
        assert!(result.contains("***"));

        // Test HTTP URL with credentials
        let input = "wget http://admin:secret@example.com/file.zip";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("secret"));
        assert!(result.contains("***"));
    }

    #[test]
    fn test_mask_passwords_environment_variables() {
        // Test PASSWORD variable
        let input = "export PASSWORD=mysecretpassword";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("mysecretpassword"));
        assert!(result.contains("***"));

        // Test API_KEY variable
        let input = "API_KEY=sk-1234567890abcdef";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("sk-1234567890abcdef"));
        assert!(result.contains("***"));

        // Test TOKEN variable
        let input = "AUTH_TOKEN=bearer_token_here";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("bearer_token_here"));
        assert!(result.contains("***"));
    }

    #[test]
    fn test_mask_passwords_api_keys() {
        // Test --api-key flag
        let input = "curl --api-key sk-123456 https://api.example.com";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("sk-123456"));
        assert!(result.contains("***"));

        // Test --token flag
        let input = "git-credential-helper --token ghp_abcdef123456";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("ghp_abcdef123456"));
        assert!(result.contains("***"));
    }

    #[test]
    fn test_mask_passwords_preserves_safe_content() {
        // Test that normal content is preserved
        let input = "ls -la /home/user/documents";
        let result = mask_passwords_in_text(input);
        assert_eq!(result, input);

        // Test that normal flags are preserved
        let input = "grep -r 'pattern' /path/to/search";
        let result = mask_passwords_in_text(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_mask_passwords_multiple_in_one_line() {
        // Test multiple passwords in one command
        let input = "mysql -u root -ppass1 && psql --password=pass2";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("pass1"));
        assert!(!result.contains("pass2"));
        assert!(result.contains("***"));
    }

    #[test]
    fn test_mask_passwords_in_log_content() {
        // Test that password masking works in the full log processing pipeline
        let input = b"mysql -u root -psecretpass -h localhost\nrenlog-end-command 0\n";
        let result = process_command_log_content(input);
        let result_str = String::from_utf8_lossy(&result);
        assert!(!result_str.contains("secretpass"));
        assert!(result_str.contains("***"));
    }

    #[test]
    fn test_mask_passwords_wget_curl_user_flag() {
        // Test wget --user flag
        let input = "wget --user=admin:secret123 ftp://example.com/file";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("secret123"));
        assert!(result.contains("***"));

        // Test curl with user credentials
        let input = "curl --user username:password123 https://api.example.com";
        let result = mask_passwords_in_text(input);
        assert!(!result.contains("password123"));
        assert!(result.contains("***"));
    }
}
