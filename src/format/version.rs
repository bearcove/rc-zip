use crate::format::*;
use nom::{combinator::map, number::streaming::le_u16};
use std::fmt;

/// A zip version (either created by, or required when reading an archive).
///
/// Versions determine which features are supported by a tool, and
/// which features are required when reading a file.
///
/// For more information, see the [.ZIP Application Note](https://support.pkware.com/display/PKZIP/APPNOTE), section 4.4.2.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Version(pub u16);

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
    pub fn parse<'a>(i: &'a [u8]) -> parse::Result<'a, Self> {
        map(le_u16, |v| Self(v))(i)
    }

    /// Identifies the host system on which the zip attributes are compatible.
    pub fn host_system(&self) -> HostSystem {
        match self.host() {
            0 => HostSystem::MsDos,
            1 => HostSystem::Amiga,
            2 => HostSystem::OpenVms,
            3 => HostSystem::Unix,
            4 => HostSystem::VmCms,
            5 => HostSystem::AtariSt,
            6 => HostSystem::Os2Hpfs,
            7 => HostSystem::Macintosh,
            8 => HostSystem::ZSystem,
            9 => HostSystem::CpM,
            10 => HostSystem::WindowsNtfs,
            11 => HostSystem::Mvs,
            12 => HostSystem::Vse,
            13 => HostSystem::AcornRisc,
            14 => HostSystem::Vfat,
            15 => HostSystem::AlternateMvs,
            16 => HostSystem::BeOs,
            17 => HostSystem::Tandem,
            18 => HostSystem::Os400,
            19 => HostSystem::Osx,
            n => HostSystem::Unknown(n),
        }
    }

    /// Integer host system
    pub fn host(&self) -> u8 {
        (self.0 >> 8) as u8
    }

    /// Integer version, e.g. 45 for Zip version 4.5
    pub fn version(&self) -> u8 {
        (self.0 & 0xff) as u8
    }

    /// ZIP specification major version
    ///
    /// See APPNOTE, section 4.4.2.1
    pub fn major(&self) -> u32 {
        self.version() as u32 / 10
    }

    /// ZIP specification minor version
    ///
    /// See APPNOTE, section 4.4.2.1
    pub fn minor(&self) -> u32 {
        self.version() as u32 % 10
    }
}

/// System on which an archive was created, as encoded into a version u16.
///
/// See APPNOTE, section 4.4.2.2
#[derive(Debug)]
pub enum HostSystem {
    /// MS-DOS and OS/2 (FAT / VFAT / FAT32 file systems)
    MsDos,
    /// Amiga
    Amiga,
    /// OpenVMS
    OpenVms,
    /// UNIX
    Unix,
    /// VM/CMS
    VmCms,
    /// Atari ST
    AtariSt,
    /// OS/2 H.P.F.S
    Os2Hpfs,
    /// Macintosh (see `Osx`)
    Macintosh,
    /// Z-System
    ZSystem,
    /// CP/M
    CpM,
    /// Windows NTFS
    WindowsNtfs,
    /// MVS (OS/390 - Z/OS)
    Mvs,
    /// VSE
    Vse,
    /// Acorn Risc
    AcornRisc,
    /// VFAT
    Vfat,
    /// alternate MVS
    AlternateMvs,
    /// BeOS
    BeOs,
    /// Tandem
    Tandem,
    /// OS/400
    Os400,
    /// OS X (Darwin)
    Osx,
    /// Unknown host system
    ///
    /// Values 20 through 255 are currently unused, as of
    /// APPNOTE.TXT 6.3.6 (April 26, 2019)
    Unknown(u8),
}
