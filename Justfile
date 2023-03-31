# just manual: https://github.com/casey/just#readme

_default:
	just --list

check:
	cargo clippy --all-features --all-targets

# Run all tests locally
test *args:
	cargo nextest run {{args}}

# Run all tests with nextest and cargo-llvm-cov
ci-test:
	#!/bin/bash -eux
	source <(cargo llvm-cov show-env --export-prefix)
	cargo llvm-cov clean --workspace
	cargo nextest run --profile ci
	cargo llvm-cov report --lcov --output-path coverage.lcov
