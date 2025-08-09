use std::fmt::Write;

/// Command marker constants, similar to zenlog's pipehelper.go
pub const COMMAND_MARKER: &str = "\x1b\x01\x09\x07\x03\x02\x05renlog:";
pub const ESCAPE_CHAR: u8 = 0x1b;
pub const SEPARATOR_CHAR: u8 = b' ';

/// Command names used in shell integration
pub const START_COMMAND_NAME: &str = "renlog-start-command";
pub const END_COMMAND_NAME: &str = "renlog-end-command";

/// Convert a hex digit to its integer value
fn hex_to_int(ch: u8) -> Result<u8, String> {
    match ch {
        b'0'..=b'9' => Ok(ch - b'0'),
        b'a'..=b'f' => Ok(ch - b'a' + 10),
        _ => Err(format!("Invalid hex digit: {}", ch as char)),
    }
}

/// Add a hex digit to the output string
#[allow(dead_code)]
fn add_hex_digit(output: &mut String, value: u8) -> Result<(), std::fmt::Error> {
    if value <= 9 {
        write!(output, "{}", (b'0' + value) as char)
    } else if value <= 15 {
        write!(output, "{}", (b'a' + (value - 10)) as char)
    } else {
        panic!("Invalid hex value: {}", value);
    }
}

/// Encode a single string argument, escaping special characters
#[allow(dead_code)]
fn encode_single(arg: &str) -> Result<String, std::fmt::Error> {
    let mut result = String::new();

    for byte in arg.bytes() {
        match byte {
            b'\n' | ESCAPE_CHAR | SEPARATOR_CHAR => {
                result.push(ESCAPE_CHAR as char);
                add_hex_digit(&mut result, (byte >> 4) & 0xf)?;
                add_hex_digit(&mut result, (byte >> 0) & 0xf)?;
            }
            _ => result.push(byte as char),
        }
    }

    Ok(result)
}

/// Decode a single string argument, unescaping special characters
fn decode_single(arg: &str) -> Result<String, String> {
    let mut result = String::new();
    let bytes = arg.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == ESCAPE_CHAR {
            if i + 2 >= bytes.len() {
                return Err("Incomplete escape sequence".to_string());
            }
            let high = hex_to_int(bytes[i + 1])?;
            let low = hex_to_int(bytes[i + 2])?;
            let decoded_byte = (high << 4) | low;
            result.push(decoded_byte as char);
            i += 3;
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    Ok(result)
}

/// Encode a string array into a single command string with marker
#[allow(dead_code)]
pub fn encode_command(args: &[String]) -> Result<String, std::fmt::Error> {
    let mut result = String::from(COMMAND_MARKER);

    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            result.push(SEPARATOR_CHAR as char);
        }
        result.push_str(&encode_single(arg)?);
    }
    result.push('\n');

    Ok(result)
}

/// Try to decode command arguments from bytes, returning (success, prefix, args)
pub fn try_decode_bytes(data: &[u8]) -> (bool, Vec<u8>, Vec<String>) {
    let marker_bytes = COMMAND_MARKER.as_bytes();

    // Find the command marker
    if let Some(pos) = data
        .windows(marker_bytes.len())
        .position(|window| window == marker_bytes)
    {
        let prefix = data[..pos].to_vec();

        // Find the end of the command (newline or end of data)
        let start = pos + marker_bytes.len();
        let mut end = data.len();

        for i in start..data.len() {
            if data[i] == b'\n' {
                end = i;
                break;
            }
        }

        if start < end {
            let command_str = String::from_utf8_lossy(&data[start..end]);
            let args = decode_command_string(&command_str);
            (true, prefix, args)
        } else {
            (true, prefix, Vec::new())
        }
    } else {
        (false, Vec::new(), Vec::new())
    }
}

/// Decode a command string into individual arguments
fn decode_command_string(command: &str) -> Vec<String> {
    if command.is_empty() {
        return Vec::new();
    }

    command
        .split(SEPARATOR_CHAR as char)
        .filter_map(|arg| decode_single(arg).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_simple_command() {
        let args = vec!["echo".to_string(), "hello".to_string()];
        let encoded = encode_command(&args).unwrap();
        assert!(encoded.starts_with(COMMAND_MARKER));
        assert!(encoded.ends_with('\n'));
    }

    #[test]
    fn test_encode_decode_round_trip() {
        let original_args = vec![
            "echo".to_string(),
            "hello world".to_string(),
            "test\nwith\nnewlines".to_string(),
        ];

        let encoded = encode_command(&original_args).unwrap();
        let data = encoded.as_bytes();
        let (success, _, decoded_args) = try_decode_bytes(data);

        assert!(success);
        assert_eq!(original_args, decoded_args);
    }

    #[test]
    fn test_decode_no_marker() {
        let data = b"regular output without marker\n";
        let (success, _, _) = try_decode_bytes(data);
        assert!(!success);
    }

    #[test]
    fn test_decode_with_prefix() {
        let args = vec!["ls".to_string(), "-la".to_string()];
        let encoded = encode_command(&args).unwrap();
        let with_prefix = format!("some output before\n{}", encoded);
        let (success, prefix, decoded_args) = try_decode_bytes(with_prefix.as_bytes());

        assert!(success);
        assert_eq!(prefix, b"some output before\n");
        assert_eq!(args, decoded_args);
    }
}
