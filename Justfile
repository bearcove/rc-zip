# just manual: https://github.com/casey/just#readme

_default:
	just --list

check:
	cargo hack clippy --each-feature

doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps

# Run all tests locally
test *args:
	cargo nextest run {{args}} --all-features

# Run all tests with nextest and cargo-llvm-cov
ci-test:
    #!/bin/bash -eux
    export RUSTUP_TOOLCHAIN=nightly
    rustup component add llvm-tools
    cargo llvm-cov --version

    cargo llvm-cov show-env --branch --export-prefix > /tmp/llvm-cov-env
    echo "======= LLVM cov env ======="
    cat /tmp/llvm-cov-env
    echo "============================"
    source /tmp/llvm-cov-env

    cargo llvm-cov clean --workspace

    export RUST_LOG=trace
    cargo nextest run --release --all-features --profile ci
    export ONE_BYTE_READ=1
    cargo nextest run --release --all-features --profile ci

    cargo llvm-cov report --release --ignore-filename-regex 'corpus/mod\.rs$' --lcov --output-path coverage.lcov
    cargo llvm-cov report --release --ignore-filename-regex 'corpus/mod\.rs$' --html
