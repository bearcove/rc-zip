# just manual: https://github.com/casey/just#readme

_default:
	just --list

check:
	cargo hack clippy --feature-powerset --group-features deflate,deflate64,lzma,bzip2

# Run all tests locally
test *args:
	cargo nextest run {{args}} --all-features

# Run all tests with nextest and cargo-llvm-cov
ci-test:
	#!/bin/bash -eux
	source <(cargo llvm-cov show-env --export-prefix)
	cargo llvm-cov clean --workspace
	cargo nextest run --all-features --profile ci
	cargo llvm-cov report --lcov --output-path coverage.lcov
