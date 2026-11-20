//! JavaScript strings count UTF-16 code units; Rust strings are UTF-8 bytes. The UI sends and receives
//! UTF-16 offsets, and everything inside this crate works on byte offsets: convert only at the boundary.

/// UTF-16 offset -> byte offset (clamped to the end of the string, and to a character boundary).
pub fn utf16_to_byte(s: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        if units >= utf16 {
            return i;
        }
        units += c.len_utf16();
    }
    s.len()
}

/// Byte offset -> UTF-16 offset.
pub fn byte_to_utf16(s: &str, byte: usize) -> usize {
    let byte = byte.min(s.len());
    s.char_indices().take_while(|(i, _)| *i < byte).map(|(_, c)| c.len_utf16()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_around_astral_characters() {
        let s = "a😀b";                       // 😀 is 4 bytes, 2 UTF-16 units
        assert_eq!(utf16_to_byte(s, 0), 0);
        assert_eq!(utf16_to_byte(s, 1), 1);
        assert_eq!(utf16_to_byte(s, 3), 5);
        assert_eq!(utf16_to_byte(s, 4), 6);
        assert_eq!(utf16_to_byte(s, 99), 6);
        assert_eq!(byte_to_utf16(s, 5), 3);
        assert_eq!(byte_to_utf16(s, 6), 4);
        assert_eq!(utf16_to_byte("héllo", 2), 3);
    }
}
