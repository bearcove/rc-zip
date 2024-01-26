use crate::{fields, format::parse};

use chrono::{
    offset::{LocalResult, TimeZone, Utc},
    DateTime, Timelike,
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
    pub fn parse(i: &[u8]) -> parse::Result<'_, Self> {
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
            let d = (self.date & 0b1_1111) as u32;
            // bits 5-8: month (1 = january, 2 = february and so on)
            let m = ((self.date >> 5) & 0b1111) as u32;
            // bits 9-15: year offset from 1980
            let y = ((self.date >> 9) + 1980) as i32;
            Utc.with_ymd_and_hms(y, m, d, 0, 0, 0)
        } {
            LocalResult::Single(date) => date,
            _ => return None,
        };

        // bits 0-4: second divided by 2
        let s = (self.time & 0b1_1111) as u32 * 2;
        // bits 5-10: minute (0-59)
        let m = (self.time >> 5 & 0b11_1111) as u32;
        // bits 11-15: hour (0-23 on a 24-hour clock)
        let h = (self.time >> 11) as u32;
        date.with_hour(h)?.with_minute(m)?.with_second(s)
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
    pub fn parse(i: &[u8]) -> parse::Result<'_, Self> {
        map(le_u64, |timestamp| Self { timestamp })(i)
    }

    /// Attempts to convert to a chrono UTC date time
    pub fn to_datetime(&self) -> Option<DateTime<Utc>> {
        // windows timestamp resolution
        let ticks_per_second = 10_000_000;
        let secs = (self.timestamp / ticks_per_second) as i64;
        let nsecs = (1_000_000_000 / ticks_per_second) * (self.timestamp * ticks_per_second);
        let epoch = Utc.with_ymd_and_hms(1601, 1, 1, 0, 0, 0).single()?;
        match Utc.timestamp_opt(epoch.timestamp() + secs, nsecs as u32) {
            LocalResult::Single(date) => Some(date),
            _ => None,
        }
    }
}

pub(crate) fn zero_datetime() -> chrono::DateTime<chrono::offset::Utc> {
    chrono::DateTime::from_naive_utc_and_offset(
        chrono::naive::NaiveDateTime::from_timestamp_opt(0, 0).unwrap(),
        chrono::offset::Utc,
    )
}
