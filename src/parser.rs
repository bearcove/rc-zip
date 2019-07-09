use super::{error::*, types::*};
use positioned_io::ReadAt;

use nom::{
    bytes::complete::tag,
    combinator::map,
    error::ParseError,
    multi::length_data,
    number::complete::{le_u16, le_u32},
    sequence::{preceded, tuple},
    IResult,
};

// Reference code for zip handling:
// https://github.com/itchio/arkive/blob/master/zip/reader.go

const DIRECTORY_END_LEN: usize = 22; // + comment

// Parses an "End of central directory record" (section 4.3.16 of APPNOTE)
fn end_of_central_directory_record<'a, E: ParseError<&'a [u8]>>(
    i: &'a [u8],
) -> IResult<&'a [u8], EndOfCentralDirectoryRecord, E> {
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
                comment: ZipString(t.6.into()),
            },
        ),
    );
    p(i)
}

fn find_signature_in_block(b: &[u8]) -> Option<(usize, EndOfCentralDirectoryRecord)> {
    for i in (0..(b.len() - DIRECTORY_END_LEN + 1)).rev() {
        let slice = &b[i..];

        if let Ok((_, directory)) = end_of_central_directory_record::<DecodingError>(slice) {
            return Some((i, directory));
        }
    }
    None
}

fn find_end_of_central_directory_record<R: ReadAt>(
    reader: &R,
    size: usize,
) -> Result<Option<(usize, EndOfCentralDirectoryRecord)>, Error> {
    let ranges: [usize; 2] = [1024, 65 * 1024];
    for &b_len in &ranges {
        let b_len = std::cmp::min(b_len, size);
        let mut buf = vec![0; b_len];
        reader.read_exact_at((size - b_len) as u64, &mut buf)?;

        if let Some((offset, directory)) = find_signature_in_block(&buf[..]) {
            let offset = size - b_len + offset;
            return Ok(Some((offset, directory)));
        }
    }
    Ok(None)
}

pub(crate) fn read_end_of_central_directory<R: ReadAt>(
    reader: &R,
    size: usize,
) -> Result<EndOfCentralDirectory, Error> {
    let (_directory_end_offset, directory_end) =
        find_end_of_central_directory_record(reader, size)?
            .ok_or(FormatError::DirectoryEndSignatureNotFound)?;

    Ok(EndOfCentralDirectory {
        directory_end,
        directory64_end: None,
        start_skip_len: 0,
    })
}
