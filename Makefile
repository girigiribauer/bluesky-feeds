test: fmt lint test-all

dev:
	./scripts/dev.sh

test-all:
	cargo test --workspace

test-integration:
	cargo test --test integration

test-unit:
	cargo test --lib

check:
	cargo check --workspace

fmt:
	cargo fmt

lint:
	cargo clippy --workspace

live-check:
	cargo run -p jetstream --example live_check

publish:
	cargo run --bin publish_feed $(FEED)

unpublish:
	cargo run --bin unpublish_feed $(FEED)

check-image:
	cargo run --bin check_image $(IMAGE)
