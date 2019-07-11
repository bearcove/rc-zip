# rc-zip

[![Build Status](https://travis-ci.org/rust-compress/rc-zip.svg?branch=master)](https://travis-ci.org/rust-compress/rc-zip)
![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)

### Intent

The goals are as follows:

  * Pure rust only
    * No C bindings allowed
    * Target platforms are Windows (MVSC), Linux, macOS, and wasm32-unknown-unknown
  * Be as compatible as possible, including:
    * ZIP64 extensions
    * Non-UTF8 file names, like CP-437 and Shift-JIS
    * Various date & time formats
    * Arbitrary files with a trailing zip
  * Have a flexible API
    * Bring your own I/O layer (sync, nonblocking, async)
    * Always allow enumerating files, even if the compression method is unsupported
    * Provide adapters for simple (and correct) usage, including permissions & symlink handling
  * No manual parsing
    * Use [nom](https://crates.io/crates/nom) instead

### Status

As of 2019-07-11, rc-zip does:

  * Find and read the end of central directory record
  * Detect and read the end of central directory record for zip64
  * Read file headers from the central directory
  * Detect character encoding for file names and comments (among UTF-8,
  CP-437, and Shift-JIS) and convert those to UTF-8 for consumption
  * Accept zip files that have leading data (like Mojosetup installers)

Up next:

  * Support zip64 compressed size, uncompressed size, and header offset
  * Decode the _many_ date & time variants (see test zips)
  * Decode file mode (unix, macOS, NTFS/VFAT/FAT)
    * Add getters for is_dir(), etc.

### Inspirations

Go's `archive/zip` package, which is extremely compatible, is used as a reference:

  * <https://golang.org/pkg/archive/zip/>

...except when it comes to API design, because Go and Rust are different beasts entirely.
  
### License

rc-zip is released under the MIT License. See the [LICENSE](LICENSE) file for details.

