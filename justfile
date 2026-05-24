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

