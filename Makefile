# Run all checks (format, lint, and all tests)
test: fmt lint test-all

# Run local dev server (loads .env)
dev:
	mkdir -p data
	set -a && . ./.env && set +a && RUST_LOG=info cargo run --bin bluesky-feeds

# Run all tests (Unit + Integration)
test-all:
	cargo test

# Run only integration tests
test-integration:
	cargo test --test integration

# Run only unit tests (lib)
test-unit:
	cargo test --lib

check:
	cargo check

fmt:
	cargo fmt

lint:
	cargo clippy

# Publish/Unpublish specific feeds
# Usage: make publish FEED=helloworld
publish:
	cargo run --bin publish_feed $(FEED)

unpublish:
	cargo run --bin unpublish_feed $(FEED)

# Check Fake Bluesky image
# Usage: make check-image IMAGE=path/to/image.jpg
check-image:
	cargo run --bin check_image $(IMAGE)
