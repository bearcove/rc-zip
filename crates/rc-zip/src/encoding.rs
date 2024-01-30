use std::fmt;

/// Encodings supported by this crate
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encoding {
    /// UTF-8
    Utf8,
    /// [Codepage 437](https://en.wikipedia.org/wiki/Code_page_437), also known as
    /// OEM-US, PC-8, or DOS Latin US.
    ///
    /// This is the fallback if UTF-8 is not specified and no other encoding
    /// is auto-detected. It was the original encoding of the zip format.
    Cp437,
    /// [Shift JIS](https://en.wikipedia.org/wiki/Shift_JIS), also known as SJIS.
    ///
    /// Still in use by some Japanese users as of 2019.
    ShiftJis,
}

impl fmt::Display for Encoding {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Encoding as T;
        match self {
            T::Utf8 => write!(f, "utf-8"),
            T::Cp437 => write!(f, "cp-437"),
            T::ShiftJis => write!(f, "shift-jis"),
        }
    }
}

/// Errors encountered while converting text to UTF-8.
#[derive(Debug, thiserror::Error)]
pub enum DecodingError {
    /// Text claimed to be UTF-8, but wasn't (as far as we can tell).
    #[error("invalid utf-8: {0}")]
    Utf8Error(std::str::Utf8Error),

    /// Text is too large to be converted.
    ///
    /// In practice, this happens if the text's length is larger than
    /// [usize::MAX], which seems unlikely.
    #[error("text too large to be converted")]
    StringTooLarge,

    #[error("encoding error: {0}")]
    EncodingError(&'static str),
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
            Encoding::Cp437 => Ok(oem_cp::decode_string_complete_table(
                i,
                &oem_cp::code_table::DECODING_TABLE_CP437,
            )),
            Encoding::ShiftJis => self.decode_as(i, encoding_rs::SHIFT_JIS),
        }
    }

    fn decode_as(
        &self,
        i: &[u8],
        encoding: &'static encoding_rs::Encoding,
    ) -> Result<String, DecodingError> {
        let mut decoder = encoding.new_decoder();
        let len = decoder
            .max_utf8_buffer_length(i.len())
            .ok_or(DecodingError::StringTooLarge)?;
        let mut v = vec![0u8; len];
        let last = true;
        let (_decoder_result, _decoder_read, decoder_written, had_errors) =
            decoder.decode_to_utf8(i, &mut v, last);
        if had_errors {
            return Err(DecodingError::EncodingError(encoding.name()));
        }
        v.resize(decoder_written, 0u8);
        Ok(unsafe { String::from_utf8_unchecked(v) })
    }
}

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
