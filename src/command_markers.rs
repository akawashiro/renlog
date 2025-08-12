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

        for (i, &byte) in data[start..].iter().enumerate() {
            if byte == b'\n' {
                end = start + i;
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
    fn test_decode_no_marker() {
        let data = b"regular output without marker\n";
        let (success, _, _) = try_decode_bytes(data);
        assert!(!success);
    }
}
