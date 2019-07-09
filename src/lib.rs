use hex_fmt::HexFmt;
use positioned_io::ReadAt;
use pretty_hex::PrettyHex;
use std::{error, fmt};

use nom::{
    bytes::complete::tag,
    combinator::map,
    error::ParseError,
    multi::length_data,
    number::complete::{le_u16, le_u32},
    sequence::{preceded, tuple},
    IResult,
};

type DecodingError<'a> = (&'a [u8], nom::error::ErrorKind);

#[derive(Debug)]
pub enum Error<'a> {
    IO(std::io::Error),
    Decoding(DecodingError<'a>),
    Format(FormatError),
    String(String),
}

#[derive(Debug)]
pub enum FormatError {
    DirectoryEndSignatureNotFound,
}

impl<'a> error::Error for Error<'a> {}

impl<'a> fmt::Display for Error<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::IO(e) => write!(f, "rc-zip IO error: {}", e),
            Error::Decoding(e) => write!(f, "rc-zip decoding error: {:#?}", e),
            Error::Format(e) => write!(f, "rc-zip error: invalid zip file: {:#?}", e),
            Error::String(e) => write!(f, "rc-zip error: {}", e),
        }
    }
}

impl<'a> From<std::io::Error> for Error<'a> {
    fn from(e: std::io::Error) -> Self {
        Error::IO(e)
    }
}

impl<'a> From<FormatError> for Error<'a> {
    fn from(e: FormatError) -> Self {
        Error::Format(e)
    }
}

impl<'a> From<DecodingError<'a>> for Error<'a> {
    fn from(e: DecodingError<'a>) -> Self {
        Error::Decoding(e)
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
    // Reference code: https://github.com/itchio/arkive/blob/master/zip/reader.go
    fn find_signature_in_block(b: &[u8]) -> Option<usize> {
        for i in (0..(b.len() - DIRECTORY_END_LEN + 1)).rev() {
            let slice = &b[i..];

            if let Ok((_, directory)) = end_of_central_directory_record::<DecodingError>(slice) {
                println!("============================================");
                println!("parsed: {:?}", slice.hex_dump());
                println!("into: {:#?}", directory);
                return Some(i);
            }
        }
        None
    }

    #[derive(Debug)]
    struct EndOfCentralDirectoryRecord<'a> {
        disk_nbr: u16,
        dir_disk_nbr: u16,
        dir_records_this_disk: u16,
        directory_records: u16,
        directory_size: u32,
        directory_offset: u32,
        comment: ZipString<'a>,
    }

    struct ZipString<'a>(&'a [u8]);

    impl<'a> fmt::Debug for ZipString<'a> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match std::str::from_utf8(self.0) {
                Ok(s) => write!(f, "{:?}", s),
                Err(_) => write!(f, "[non-utf8 string: {:x}]", HexFmt(self.0)),
            }
        }
    }

    fn end_of_central_directory_record<'a, E: ParseError<&'a [u8]>>(
        i: &'a [u8],
    ) -> IResult<&'a [u8], EndOfCentralDirectoryRecord<'a>, E> {
        let p = preceded(
            tag("PK\x05\x06"),
            map(
                tuple((
                    le_u16,
                    le_u16,
                    le_u16,
                    le_u16,
                    le_u32,
                    le_u32,
                    length_data(le_u16),
                )),
                |t| EndOfCentralDirectoryRecord {
                    disk_nbr: t.0,
                    dir_disk_nbr: t.1,
                    dir_records_this_disk: t.2,
                    directory_records: t.3,
                    directory_size: t.4,
                    directory_offset: t.5,
                    comment: ZipString(t.6),
                },
            ),
        );
        p(i)
    }

    fn find_signature(reader: &dyn ReadAt, size: usize) -> Result<Option<usize>, Error> {
        let ranges: [usize; 2] = [1024, 65 * 1024];
        for &b_len in &ranges {
            let b_len = std::cmp::min(b_len, size);
            let mut buf = vec![0; b_len];
            reader.read_exact_at((size - b_len) as u64, &mut buf)?;

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
