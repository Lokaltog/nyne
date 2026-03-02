# Format, lint, and check the project
lint:
    cargo fmt
    cargo clippy --fix --allow-dirty --message-format=short -- -D warnings

# Full pre-commit verification
check: lint
    # cargo deny check
    cargo check --message-format=short
    cargo nextest run

# Run tests only
test *ARGS:
    cargo nextest run {{ARGS}}

# Supply chain audit (licenses, advisories, bans)
deny:
    cargo deny check

# Review insta snapshots
review:
    cargo insta review

# Run benchmarks
bench *ARGS:
    cargo bench {{ARGS}}

# Build and open documentation
doc:
    cargo doc --workspace --no-deps --open

# Watch for changes and re-check
watch:
    cargo watch -x 'check --message-format=short'

# Find unused dependencies
machete:
    cargo machete --with-metadata

# Clean build artifacts
clean:
    cargo clean
