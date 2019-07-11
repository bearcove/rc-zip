use std::fmt;

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

#[derive(Clone, Copy)]
pub enum Encoding {
    Utf8,
    Cp437,
    Other(OtherEncoding),
}

impl fmt::Display for Encoding {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Encoding as T;
        match self {
            T::Utf8 => write!(f, "utf-8"),
            T::Cp437 => write!(f, "cp-437"),
            T::Other(other) => write!(f, "{}", other),
        }
    }
}

#[derive(Clone, Copy)]
pub enum OtherEncoding {
    ShiftJis,
}

impl fmt::Display for OtherEncoding {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use OtherEncoding as T;
        match self {
            T::ShiftJis => write!(f, "shift-jis"),
        }
    }
}

#[derive(Debug)]
pub enum DecodingError {
    Utf8Error(std::str::Utf8Error),
}

impl From<std::str::Utf8Error> for DecodingError {
    fn from(e: std::str::Utf8Error) -> Self {
        DecodingError::Utf8Error(e)
    }
}

impl Encoding {
    pub(crate) fn decode(&self, i: &[u8]) -> Result<String, DecodingError> {
        match self {
            Encoding::Utf8 => {
                let s = std::str::from_utf8(i)?;
                Ok(s.to_string())
            }
            Encoding::Cp437 => {
                use codepage_437::{BorrowFromCp437, CP437_CONTROL};
                let s = String::borrow_from_cp437(i, &CP437_CONTROL);
                Ok(s.to_string())
            }
            Encoding::Other(o) => o.decode(i),
        }
    }
}

impl OtherEncoding {
    // FIXME: don't panic
    fn decode(&self, i: &[u8]) -> Result<String, DecodingError> {
        let encoding = match self {
            OtherEncoding::ShiftJis => encoding_rs::SHIFT_JIS,
        };

        let mut decoder = encoding.new_decoder();
        // FIXME: don't panic
        let len = decoder
            .max_utf8_buffer_length(i.len())
            .expect("decoded string should fit into usize");
        let mut v = vec![0u8; len];
        let last = true;
        let (_decoder_result, _decoder_read, decoder_written, had_errors) =
            decoder.decode_to_utf8(i, &mut v, last);
        if had_errors {
            // FIXME: don't panic
            panic!("could not decode encoding {}", encoding.name());
        }
        v.resize(decoder_written, 0u8);
        Ok(unsafe { String::from_utf8_unchecked(v) })
    }
}
