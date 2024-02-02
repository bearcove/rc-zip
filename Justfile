# just manual: https://github.com/casey/just#readme

_default:
	just --list

check:
	cargo hack clippy --each-feature

docs:
	RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps

# Run all tests locally
test *args:
	cargo nextest run {{args}} --all-features

# Report unused dependencies:
udeps:
	RUSTC_BOOTSTRAP=1 cargo udeps

# Run all tests with nextest and cargo-llvm-cov
ci-test:
	#!/bin/bash -eux
	source <(cargo llvm-cov show-env --export-prefix)
	cargo llvm-cov clean --workspace
	cargo nextest run --all-features --profile ci
	cargo llvm-cov report --lcov --output-path coverage.lcov
