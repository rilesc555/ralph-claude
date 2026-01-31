//! Keyboard input forwarding to PTY.

use std::io::Write;
use crossterm::event::{KeyCode, KeyModifiers};

/// Forward a key event to the PTY writer
///
/// Converts crossterm key events to the appropriate byte sequences for the terminal.
/// Returns true if bytes were written, false if the key is not handled.
pub fn forward_key_to_pty<W: Write>(writer: &mut W, key_code: KeyCode, modifiers: KeyModifiers) -> bool {
    let bytes: Vec<u8> = match key_code {
        // Printable characters
        KeyCode::Char(c) => {
            if modifiers.contains(KeyModifiers::CONTROL) {
                // Handle Ctrl+key combinations
                // Ctrl+A = 0x01, Ctrl+B = 0x02, ..., Ctrl+Z = 0x1A
                // Ctrl+C = 0x03 (interrupt)
                if c.is_ascii_alphabetic() {
                    let ctrl_char = (c.to_ascii_lowercase() as u8) - b'a' + 1;
                    vec![ctrl_char]
                } else if c == '[' {
                    vec![0x1b] // Escape
                } else if c == '\\' {
                    vec![0x1c] // File separator (Ctrl+\)
                } else if c == ']' {
                    vec![0x1d] // Group separator (Ctrl+])
                } else if c == '^' {
                    vec![0x1e] // Record separator (Ctrl+^)
                } else if c == '_' {
                    vec![0x1f] // Unit separator (Ctrl+_)
                } else {
                    // Just send the character for other Ctrl combinations
                    c.to_string().into_bytes()
                }
            } else if modifiers.contains(KeyModifiers::ALT) {
                // Alt+key sends ESC followed by the character
                let mut bytes = vec![0x1b]; // ESC
                bytes.extend(c.to_string().into_bytes());
                bytes
            } else {
                // Regular character
                c.to_string().into_bytes()
            }
        }

        // Special keys
        KeyCode::Enter => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                // Shift+Enter: send newline for multi-line input
                // Some terminals use CSI 13;2u for modified Enter
                vec![0x1b, b'[', b'1', b'3', b';', b'2', b'u']
            } else {
                vec![b'\r'] // Regular Enter: carriage return
            }
        }
        KeyCode::Backspace => vec![0x7f],  // DEL character (most terminals)
        KeyCode::Delete => vec![0x1b, b'[', b'3', b'~'], // ANSI escape sequence
        KeyCode::Tab => vec![b'\t'],       // Tab character

        // Arrow keys (ANSI escape sequences)
        KeyCode::Up => vec![0x1b, b'[', b'A'],
        KeyCode::Down => vec![0x1b, b'[', b'B'],
        KeyCode::Right => vec![0x1b, b'[', b'C'],
        KeyCode::Left => vec![0x1b, b'[', b'D'],

        // Home/End keys
        KeyCode::Home => vec![0x1b, b'[', b'H'],
        KeyCode::End => vec![0x1b, b'[', b'F'],

        // Page Up/Down
        KeyCode::PageUp => vec![0x1b, b'[', b'5', b'~'],
        KeyCode::PageDown => vec![0x1b, b'[', b'6', b'~'],

        // Insert key
        KeyCode::Insert => vec![0x1b, b'[', b'2', b'~'],

        // Function keys
        KeyCode::F(1) => vec![0x1b, b'O', b'P'],
        KeyCode::F(2) => vec![0x1b, b'O', b'Q'],
        KeyCode::F(3) => vec![0x1b, b'O', b'R'],
        KeyCode::F(4) => vec![0x1b, b'O', b'S'],
        KeyCode::F(5) => vec![0x1b, b'[', b'1', b'5', b'~'],
        KeyCode::F(6) => vec![0x1b, b'[', b'1', b'7', b'~'],
        KeyCode::F(7) => vec![0x1b, b'[', b'1', b'8', b'~'],
        KeyCode::F(8) => vec![0x1b, b'[', b'1', b'9', b'~'],
        KeyCode::F(9) => vec![0x1b, b'[', b'2', b'0', b'~'],
        KeyCode::F(10) => vec![0x1b, b'[', b'2', b'1', b'~'],
        KeyCode::F(11) => vec![0x1b, b'[', b'2', b'3', b'~'],
        KeyCode::F(12) => vec![0x1b, b'[', b'2', b'4', b'~'],
        KeyCode::F(_) => return false, // Unsupported function keys

        // Escape key - send raw ESC byte
        KeyCode::Esc => vec![0x1b],

        // Other keys we don't handle
        _ => return false,
    };

    let _ = writer.write_all(&bytes);
    let _ = writer.flush();
    true
}
