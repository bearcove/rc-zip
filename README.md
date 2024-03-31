# rc-zip

[![test pipeline](https://github.com/bearcove/rc-zip/actions/workflows/test.yml/badge.svg)](https://github.com/bearcove/rc-zip/actions/workflows/test.yml?query=branch%3Amain)
[![Coverage Status (codecov.io)](https://codecov.io/gh/bearcove/rc-zip/branch/main/graph/badge.svg)](https://codecov.io/gh/bearcove/rc-zip/)

### Motivation

Have a pure rust, highly compatible, I/O-model-independent, zip reading and
writing library.

(Note: as of now, rc-zip does reading only)

### Funding

Thanks to these companies for contracting work on rc-zip:

[![Row Zero](./static/row-zero.svg)](https://rowzero.io)

And thanks to all my [individual sponsors](https://fasterthanli.me/donate).

### Design decisions

The core of this crate does not perform I/O directly. Instead, it uses a state
machine, and asks for reads at specific offsets. This allows it to work under
different I/O models: blocking, non-blocking, and async. It has no expectations
of the zip archive being present on disk (ie. it doesn't assume `std::fs`), just
that random access is possible.

The recommended interface relies on the central directory rather than local headers:

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

However, as of v4.0.0, a streaming decompression interface was added to both
`rc-zip-sync` and `rc-zip-tokio`, in the form of the `ReadZipStreaming` traits.

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

As much as possible, [winnow](https://crates.io/crates/winnow) is used to parse the
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
  * Not everyone wants a compliant zip reader. Some may want to rely on local
  headers instead, completely ignoring the central directory, thus throwing
  caution to the wind.

As a result, the structs in this crate are state machines, that advertise their
need to read (and from where), to process data, or to write. I/O errors are
cleanly separated from the rest, and calls to this crate never block.

Separate crates add specific I/O models on top of rc-zip, see the [rc-zip-sync](https://crates.io/crates/rc-zip-sync)
and [rc-zip-tokio](https://crates.io/crates/rc-zip-tokio) crates.

## License

This project is primarily distributed under the terms of both the MIT license
and the Apache License (Version 2.0).

See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT) for details.
