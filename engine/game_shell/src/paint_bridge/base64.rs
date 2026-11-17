//! Base64 (RFC 4648, the standard alphabet, with `=` padding): how a picture's pixels go in a line of text. Written here because nothing else in the engine needs it.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = u32::from(chunk[0]) << 16
            | u32::from(chunk.get(1).copied().unwrap_or(0)) << 8
            | u32::from(chunk.get(2).copied().unwrap_or(0));
        for position in 0..4 {
            if position <= chunk.len() {
                text.push(ALPHABET[(word >> (18 - 6 * position) & 63) as usize] as char);
            } else {
                text.push('=');
            }
        }
    }
    text
}

fn value_of(character: u8) -> Option<u32> {
    match character {
        b'A'..=b'Z' => Some(u32::from(character - b'A')),
        b'a'..=b'z' => Some(u32::from(character - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(character - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// The bytes a text stands for, or `None` if it is not base64 (a character outside the alphabet, a length that is not a multiple of four, padding anywhere but the end).
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let characters = text.as_bytes();
    if !characters.len().is_multiple_of(4) {
        return None;
    }
    let mut bytes = Vec::with_capacity(characters.len() / 4 * 3);
    let last = characters.len() / 4;
    for (index, chunk) in characters.chunks(4).enumerate() {
        let padding = chunk
            .iter()
            .rev()
            .take_while(|&&character| character == b'=')
            .count();
        if padding > 2 || (padding > 0 && index + 1 != last) {
            return None;
        }
        let mut word = 0u32;
        for (position, &character) in chunk.iter().enumerate() {
            let value = if position >= 4 - padding {
                0
            } else {
                value_of(character)?
            };
            word |= value << (18 - 6 * position);
        }
        for position in 0..3 - padding {
            bytes.push((word >> (16 - 8 * position)) as u8);
        }
    }
    Some(bytes)
}
