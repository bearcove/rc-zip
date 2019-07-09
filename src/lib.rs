use positioned_io::ReadAt;
use pretty_hex::PrettyHex;
use std::{error, fmt};

#[derive(Debug)]
pub enum Error {
    IO(std::io::Error),
    Format(FormatError),
    String(String),
}

#[derive(Debug)]
pub enum FormatError {
    DirectoryEndSignatureNotFound,
}

impl error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::IO(e) => write!(f, "rc-zip IO error: {}", e),
            Error::Format(e) => write!(f, "rc-zip error: invalid zip file: {:#?}", e),
            Error::String(e) => write!(f, "rc-zip error: {}", e),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IO(e)
    }
}

impl From<FormatError> for Error {
    fn from(e: FormatError) -> Self {
        Error::Format(e)
    }
}

pub struct ZipReader<'a> {
    reader: &'a dyn ReadAt,
    size: usize,
}

impl<'a> ZipReader<'a> {
    fn entries(&self) -> &[ZipEntry<'a>] {
        unimplemented!()
    }
}

pub struct ZipEntry<'a> {
    name: &'a str,
}

impl<'a> ZipEntry<'a> {
    fn name() -> &'a str {
        unimplemented!()
    }
}

const DIRECTORY_END_LEN: usize = 22; // + comment

pub fn open<'a>(reader: &'a dyn ReadAt, size: usize) -> Result<ZipReader<'a>, Error> {
    fn find_signature_in_block(b: &[u8]) -> Option<usize> {
        for i in (0..(b.len() - DIRECTORY_END_LEN + 1)).rev() {
            if b[i] == ('P' as u8)
                && b[i + 1] == ('K' as u8)
                && b[i + 2] == 0x05
                && b[i + 3] == 0x06
            {
                // n is length of comment
                let n = (b[i + DIRECTORY_END_LEN - 2] as usize)
                    | ((b[i + DIRECTORY_END_LEN - 1] as usize) << 8);
                if n + DIRECTORY_END_LEN < b.len() {
                    return Some(i);
                }
            }
        }
        None
    }

    fn find_signature(reader: &dyn ReadAt, size: usize) -> Result<Option<usize>, Error> {
        let ranges: [usize; 2] = [1024, 65 * 1024];
        for &b_len in &ranges {
            let b_len = if b_len > size { size } else { b_len };
            let mut buf = vec![0; b_len];
            reader.read_exact_at((size - b_len) as u64, &mut buf)?;

            println!("buf = {:?}", buf.hex_dump());

            if let Some(p) = find_signature_in_block(&buf[..]) {
                return Ok(Some(size - b_len + p));
            }
        }
        Ok(None)
    }

    let offset = find_signature(reader, size)?.ok_or(FormatError::DirectoryEndSignatureNotFound)?;
    println!("end signature offset = {}", offset);

    Ok(ZipReader { reader, size })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_test_files() {
        use std::path::PathBuf;
        let zips_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("test-zips");

        for name in &["test.zip", "zip64.zip", "unix.zip", "winxp.zip", "dd.zip"] {
            let test_file = zips_dir.join(name);
            let contents = std::fs::read(test_file).unwrap();
            super::open(&contents, contents.len()).unwrap();
        }
    }
}
