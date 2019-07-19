# rc-zip

[![Build Status](https://travis-ci.org/rust-compress/rc-zip.svg?branch=master)](https://travis-ci.org/rust-compress/rc-zip)
![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)

### Motivation

Have a pure rust, highly compatible, I/O-model independent, zip reading and
writing library.

### Design decisions

This crate does not perform I/O directly. Instead, it uses a state machine, and asks
for reads at specific offsets. This allows it to work under different I/O models:
blocking, non-blocking, and async. It has no expectations of the zip archive being
present on disk (ie. it doesn't assume `std::fs`), just that random access is possible.

This crate relies fully on the central directory, not on local headers:

```
[local file header 1] // <---------------- ignored
[file data 1]
[local file header 2]
[file data 2]
[central directory header 1] // <--------- used
[central directory header 2]
[end of central directory record]
```

The reason for that is that the central directory is the canonical list of
entries in a zip. Archives that have been repacked may contain duplicate local
file headers (and data), along with headers for entries that have been removed.
Only the central directory is authoritative when it comes to the contents of a
zip archive.

This crate accepts what is known as "trailing zips" - for example, files that
are valid ELF or PE executables, and merely have a valid zip archive appended.
This covers some forms of self-extracting archives and installers.

This crate recognizes and uses zip64 metadata. This allows for a large number
of entries (above 65536) and large entries (above 4GiB). This crate attempts to
forgives some non-standard behavior from common tools. Such behavior has been
observed in the wild and is, whenever possible, tested.

This crate attempts to recognize as much metadata as possible, and normalize
it. For example, MSDOS timestamps, NTFS timestamps, Extended timestamps and
Unix timestamps are supported, and they're all converted to a [chrono
DateTime<Utc>](https://crates.io/crates/chrono).

Although the normalized version of metadata (names, timestamps, UID, GID, etc.)
is put front and center, this crate attempts to expose a "raw" version of
that same metadata whenever the authors felt it was necessary.

Whenever the zip archive doesn't explicitly specify UTF-8 encoding, this crate
relies on encoding detection to decide between CP-437 and Shift-JIS. It uses
[encoding_rs](https://crates.io/crates/encoding_rs) to deal with Shift-JIS.

Due to the history of the zip format, some compatibility issues are to be
expected: for example, for archives with only MSDOS timestamps, the results
might be in the wrong timezone. For archive with very few files and non-UTF8
names, the encoding might not be detected properly, and thus decoding may fail.

As much as possible, [nom](https://crates.io/crates/nom) is used to parse the
various data structures used in the zip archive format. This allows a
semi-declarative style that is easier to write, read, and amend if needed.
Some (hygienic) macros are used to avoid repetition.

### API design

The design of the API is constrained by several parameters:

  * A compliant zip reader *must* first read the central directory, located
  near the end of the zip archive. This means simply taking an `Read` won't do.
  * Multiple I/O models must be supported. Whereas other crates focus on
  taking a `Read`, a `Read + Seek`, or simply a byte slice, this crate aims
  to support synchronous *and* asynchronous I/O.

As a result, the structs in this crate are state machines, that advertise
their need to read (and from where), to process data, or to write. As a
result, I/O errors are cleanly separated from the rest, and calls to this
crate never block.

See the inline rustdoc comments for more details on API design.

### License

rc-zip is released under the MIT License. See the [LICENSE](LICENSE) file for details.

