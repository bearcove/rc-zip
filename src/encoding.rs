// detect_utf8 reports whether s is a valid UTF-8 string, and whether the string
// must be considered UTF-8 encoding (i.e., not compatible with CP-437, ASCII,
// or any other common encoding).
pub(crate) fn detect_utf8(input: &[u8]) -> (bool, bool) {
    match std::str::from_utf8(input) {
        Err(_) => {
            // not valid utf-8
            (false, false)
        }
        Ok(s) => {
            let mut require = false;

            // Officially, ZIP uses CP-437, but many readers use the system's
            // local character encoding. Most encoding are compatible with a large
            // subset of CP-437, which itself is ASCII-like.
            //
            // Forbid 0x7e and 0x5c since EUC-KR and Shift-JIS replace those
            // characters with localized currency and overline characters.
            for c in s.chars() {
                if c < 0x20 as char || c > 0x7d as char || c == 0x5c as char {
                    require = true
                }
            }
            (true, require)
        }
    }
}
