set positional-arguments

# Run the standard local quality gate without mutating source files.
check:
    cargo +nightly fmt --all --check
    cargo +nightly clippy --all-targets --all-features -- -D warnings
    cargo test

# Apply formatting and Clippy autofixes where available.
fix:
    cargo +nightly fmt --all
    cargo +nightly clippy --all-targets --all-features --fix --allow-dirty --allow-staged -- -D warnings

# Fast iteration path when full tests are unnecessary.
lint:
    cargo +nightly fmt --all --check
    cargo +nightly clippy --all-targets --all-features -- -D warnings

# Install the repository-managed Git hooks directory for this clone.
install-hooks:
    git config core.hooksPath .githooks

# Build feature bundles defined in Cargo.toml. Useful for verifying that
# every feature combination still compiles after changes to module gating.
check-minimal:
    cargo +nightly clippy --no-default-features --features minimal --all-targets -- -D warnings

check-standard:
    cargo +nightly clippy --no-default-features --features standard --all-targets -- -D warnings

check-full:
    cargo +nightly clippy --all-features --all-targets -- -D warnings

# Private/profiling features that should never be enabled on their own.
_hack_excluded := "hotpath,hotpath-alloc,_tls-base,_tls-client,_tls-server,_http-server,_http-client"

# Compile every public feature individually (and the bare no-default-features
# core). Catches "feature X fails to build in isolation" regressions.
# Requires cargo-hack: `cargo install cargo-hack`.
check-each-feature:
    cargo +nightly hack check --each-feature --no-dev-deps --exclude-features {{_hack_excluded}}

# Pairwise (depth-2) powerset of the granular features. Slower; mirrors the
# nightly CI job. Bundles are unions, so they are excluded.
check-powerset:
    cargo +nightly hack check --feature-powerset --depth 2 --no-dev-deps \
      --exclude-features minimal,standard,full,{{_hack_excluded}}

# Run the test suite under the slim bundles so feature-gated tests
# (tests/feature_gating.rs negative cases, etc.) actually execute.
test-minimal:
    cargo test --no-default-features --features minimal

test-standard:
    cargo test --no-default-features --features standard

# Full local mirror of CI: per-feature compile sweep + the three bundle
# clippy gates + bundle tests + the all-features test suite.
check-matrix: check-each-feature check-minimal check-standard check-full test-minimal test-standard
    cargo test --all-features

