
# Changelog

## 2.0.0 (2023-03-31)

Migrate from nom 5 to nom 7, closes
[#20](https://github.com/fasterthanlime/rc-zip/issues/20).

Switch from `chardet` to `chardetng` to avoid GPL'd code, closes
[#15](https://github.com/fasterthanlime/rc-zip/issues/15).

Remove all async codepaths (this also removes dependency on the `ara` / `ubio`
crates). Those have to be rethought. The core (without the `sync` feature) is
still "sans-io" and could theoretically be used from async code (albeit by
duplicating a lot of logic, which isn't ideal).

Only depend on `positioned-io` if the `file` cargo feature (which is new) is
enabled (that's the default), which allows building for `wasm32-unknown-unknown`,
closes [#16](https://github.com/fasterthanlime/rc-zip/issues/16)

Provide a higher-level API for the `sync` feature which doesn't involve doing
the whole `get_reader` dance, closes [#5](https://github.com/fasterthanlime/rc-zip/issues/5).

### Various maintenance tasks

CI was moved from Travis CI (RIP) to GitHub Actions, code coverage is now tracked
[on codecov](https://app.codecov.io/gh/fasterthanlime/rc-zip).

Links were updated from `gihub.com/rust-compress/` to `github.com/fasterthanlime/` since the repo was
moved when we closed out the `rust-compress` organization.

All clippy lints were re-enabled and fixed. Both crates were ported from edition
2018 to edition 2021. Both crates (`rc-zip` and `jean`) now live in the
`crates/` subdirectory. `[workspace.dependencies]` is used to make sure common
dependencies are at the same version.

All dependencies have been updated, including chrono, clap, crc32fast,
encoding_rs, humansize, indicatif, libflate.

Codepage 437 decoding is now handled by the `oem_cp` crate (as opposed to
`codepage_437`, which is pretty old now).

The codebase has been migrated from the `log` crate to the `tracing` crate,
although most of the events are TRACE-level and only there for rc-zip
developers.

## 0.0.1 (2019-12-14)

Initial release.