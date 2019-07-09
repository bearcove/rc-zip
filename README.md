# rc-zip

[![Build Status](https://travis-ci.org/rust-compress/rc-zip.svg?branch=master)](https://travis-ci.org/rust-compress/rc-zip)

### Intent

The goals are as follows:

  * Pure rust only
    * No C bindings allowed
    * Everything must compile for Windows, Linux, macOS, and wasm32-unknown-unknown
  * Read as many ZIP files as possible, including:
    * ZIP64 files
    * Slightly malformed but common zip files
    * Non-UTF8 file names, like CP-437 and Shift-JIS
  * Support as much metadata as possible (even if it's not present in all zip files)
  * Pluggable decompression:
    * Always allow enumerating files, even if the compression method is unsupported
  * Allow concurrent entry readers
    * Rely on a positional I/O trait, like ReadAt (current candidate is [olio](https://crates.io/crates/olio))
  * No manual parsing
    * Use [nom](https://crates.io/crates/nom) instead

### Status

  * Wrote the statement of intent
  * The rest will follow

