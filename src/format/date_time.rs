use crate::format::parse;

use chrono::{
    offset::{LocalResult, TimeZone, Utc},
    DateTime,
};
use nom::{
    combinator::map,
    number::streaming::{le_u16, le_u64},
};
use std::fmt;

/// A timestamp in MS-DOS format
///
/// Represents dates from year 1980 to 2180, with 2 second precision.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct MsdosTimestamp {
    pub time: u16,
    pub date: u16,
}

impl fmt::Debug for MsdosTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.to_datetime() {
            Some(dt) => write!(f, "MsdosTimestamp({})", dt),
            None => write!(f, "MsdosTimestamp(?)"),
        }
    }
}

impl MsdosTimestamp {
    /// Parse an MS-DOS timestamp from a byte slice
    pub fn parse<'a>(i: &'a [u8]) -> parse::Result<'a, Self> {
        fields!(Self {
            time: le_u16,
            date: le_u16,
        })(i)
    }

    /// Attempts to convert to a chrono UTC date time
    pub fn to_datetime(&self) -> Option<DateTime<Utc>> {
        // see https://docs.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-dosdatetimetofiletime
        let date = match {
            // bits 0-4: day of the month (1-31)
            let d = (self.date & 0b1111_1) as u32;
            // bits 5-8: month (1 = january, 2 = february and so on)
            let m = ((self.date >> 5) & 0b1111) as u32;
            // bits 9-15: year offset from 1980
            let y = ((self.date >> 9) + 1980) as i32;
            Utc.ymd_opt(y, m, d)
        } {
            LocalResult::Single(date) => date,
            _ => return None,
        };

        // bits 0-4: second divided by 2
        let s = (self.time & 0b1111_1) as u32 * 2;
        // bits 5-10: minute (0-59)
        let m = (self.time >> 5 & 0b1111_11) as u32;
        // bits 11-15: hour (0-23 on a 24-hour clock)
        let h = (self.time >> 11) as u32;
        date.and_hms_opt(h, m, s)
    }
}

/// A timestamp in NTFS format.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct NtfsTimestamp {
    pub timestamp: u64,
}

impl fmt::Debug for NtfsTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.to_datetime() {
            Some(dt) => write!(f, "NtfsTimestamp({})", dt),
            None => write!(f, "NtfsTimestamp(?)"),
        }
    }
}

impl NtfsTimestamp {
    /// Parse an MS-DOS timestamp from a byte slice
    pub fn parse<'a>(i: &'a [u8]) -> parse::Result<'a, Self> {
        map(le_u64, |timestamp| Self { timestamp })(i)
    }

    /// Attempts to convert to a chrono UTC date time
    pub fn to_datetime(&self) -> Option<DateTime<Utc>> {
        // windows timestamp resolution
        let ticks_per_second = 10_000_000;
        let secs = (self.timestamp / ticks_per_second) as i64;
        let nsecs =
            (1_000_000_000 / ticks_per_second) * ((self.timestamp * ticks_per_second) as u64);
        let epoch = Utc.ymd(1601, 1, 1).and_hms(0, 0, 0);
        match Utc.timestamp_opt(epoch.timestamp() + secs, nsecs as u32) {
            LocalResult::Single(date) => Some(date),
            _ => None,
        }
    }
}

pub(crate) fn zero_datetime() -> chrono::DateTime<chrono::offset::Utc> {
    chrono::DateTime::from_utc(
        chrono::naive::NaiveDateTime::from_timestamp(0, 0),
        chrono::offset::Utc,
    )
}
