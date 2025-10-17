# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [4.2.8](https://github.com/bearcove/rc-zip/compare/rc-zip-tokio-v4.2.7...rc-zip-tokio-v4.2.8) - 2025-10-17

### Other

- updated the following local packages: rc-zip, rc-zip

## [4.2.7](https://github.com/bearcove/rc-zip/compare/rc-zip-tokio-v4.2.6...rc-zip-tokio-v4.2.7) - 2025-08-30

### Other

- updated the following local packages: rc-zip, rc-zip

## [4.2.6](https://github.com/bearcove/rc-zip/compare/rc-zip-tokio-v4.2.5...rc-zip-tokio-v4.2.6) - 2025-04-08

### Other

- *(deps)* bump tokio in the cargo group across 1 directory

## [4.2.5](https://github.com/bearcove/rc-zip/compare/rc-zip-tokio-v4.2.4...rc-zip-tokio-v4.2.5) - 2025-03-02

### Other

- updated the following local packages: rc-zip, rc-zip

## [4.2.4](https://github.com/bearcove/rc-zip/compare/rc-zip-tokio-v4.2.3...rc-zip-tokio-v4.2.4) - 2025-02-05

### Other

- updated the following local packages: rc-zip, rc-zip

## [4.2.3](https://github.com/bearcove/rc-zip/compare/rc-zip-tokio-v4.2.2...rc-zip-tokio-v4.2.3) - 2024-12-17

### Other

- updated the following local packages: rc-zip, rc-zip

## [4.2.2](https://github.com/bearcove/rc-zip/compare/rc-zip-tokio-v4.2.1...rc-zip-tokio-v4.2.2) - 2024-09-17

### Other

- Remove unnecessary deps
- Add/fix logo attribution

## [4.2.1](https://github.com/bearcove/rc-zip/compare/rc-zip-tokio-v4.2.0...rc-zip-tokio-v4.2.1) - 2024-09-05

### Other
- Update logo attribution

## [4.2.0](https://github.com/bearcove/rc-zip/compare/rc-zip-tokio-v4.1.0...rc-zip-tokio-v4.2.0) - 2024-09-04

### Added
- Add logo

## [4.1.0](https://github.com/fasterthanlime/rc-zip/compare/rc-zip-tokio-v4.0.1...rc-zip-tokio-v4.1.0) - 2024-03-19

### Added
- Measure code coverage differently ([#79](https://github.com/fasterthanlime/rc-zip/pull/79))
- futures => futures_util (fewer deps)
- Run one-byte-read tests in CI in release ([#77](https://github.com/fasterthanlime/rc-zip/pull/77))
- More efficient  implementation
- rc-zip-tokio: Re-use cursor if it's at the right offset already ([#71](https://github.com/fasterthanlime/rc-zip/pull/71))

### Fixed
- lzma_dec: count all input in outcome.bytes_read
- Don't give up on reading local header when given short reads
- fix arafc bug I just introduced

### Other
- release ([#68](https://github.com/fasterthanlime/rc-zip/pull/68))

## [4.0.1](https://github.com/fasterthanlime/rc-zip/compare/rc-zip-tokio-v4.0.0...rc-zip-tokio-v4.0.1) - 2024-03-12

### Other
- updated the following local packages: rc-zip, rc-zip

## [4.0.0](https://github.com/fasterthanlime/rc-zip/compare/rc-zip-tokio-v3.0.0...rc-zip-tokio-v4.0.0) - 2024-02-05

### Added
- [**breaking**] Introduce `ReadZipStreaming` trait ([#62](https://github.com/fasterthanlime/rc-zip/pull/62))

## [3.0.0](https://github.com/fasterthanlime/rc-zip/releases/tag/rc-zip-tokio-v3.0.0) - 2024-02-02

### Other
- Bump rc-zip-sync & rc-zip-tokio
- Introduce rc-zip-sync, rc-zip-tokio ([#60](https://github.com/fasterthanlime/rc-zip/pull/60))
