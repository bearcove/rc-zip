use num_enum::{FromPrimitive, IntoPrimitive};
use std::fmt;
use winnow::{binary::le_u8, seq, PResult, Parser, Partial};

/// A zip version (either created by, or required when reading an archive).
///
/// Versions determine which features are supported by a tool, and
/// which features are required when reading a file.
///
/// For more information, see the [.ZIP Application Note](https://support.pkware.com/display/PKZIP/APPNOTE), section 4.4.2.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Version {
    /// The host system on which
    pub host_system: HostSystem,

    /// Integer version, e.g. 45 for Zip version 4.5
    /// See APPNOTE, section 4.4.2.1
    pub version: u8,
}

impl fmt::Debug for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:?} v{}.{}",
            self.host_system(),
            self.major(),
            self.minor()
        )
    }
}

impl Version {
    /// Parse a version from a byte slice
    pub fn parser(i: &mut Partial<&'_ [u8]>) -> PResult<Self> {
        seq! {Self {
            host_system: le_u8.map(HostSystem::from_u8),
            version: le_u8,
        }}
        .parse_next(i)
    }
}

/// System on which an archive was created, as encoded into a version u16.
///
/// See APPNOTE, section 4.4.2.2
#[derive(Debug, Clone, Copy, IntoPrimitive, FromPrimitive)]
#[repr(u8)]
pub enum HostSystem {
    /// MS-DOS and OS/2 (FAT / VFAT / FAT32 file systems)
    MsDos = 0,

    /// Amiga
    Amiga = 1,

    /// OpenVMS
    OpenVms = 2,

    /// UNIX
    Unix = 3,

    /// VM/CMS
    VmCms = 4,

    /// Atari ST
    AtariSt = 5,

    /// OS/2 H.P.F.S
    Os2Hpfs = 6,

    /// Macintosh (see `Osx`)
    Macintosh = 7,

    /// Z-System
    ZSystem = 8,

    /// CP/M
    CpM = 9,

    /// Windows NTFS
    WindowsNtfs = 10,

    /// MVS (OS/390 - Z/OS)
    Mvs = 11,

    /// VSE
    Vse = 12,

    /// Acorn Risc
    AcornRisc = 13,

    /// VFAT
    Vfat = 14,

    /// alternate MVS
    AlternateMvs = 15,

    /// BeOS
    BeOs = 16,

    /// Tandem
    Tandem = 17,

    /// OS/400
    Os400 = 18,

    /// OS X (Darwin)
    Osx = 19,

    /// Unknown host system
    ///
    /// Values 20 through 255 are currently unused, as of
    /// APPNOTE.TXT 6.3.10
    #[num_enum(catch_all)]
    Unknown(u8),
}
