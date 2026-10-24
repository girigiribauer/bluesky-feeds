test: fmt lint test-all

dev:
	op run --env-file=.env.op -- ./scripts/dev.sh -p bluesky-feeds --bin bluesky-feeds

dev-consumer:
	./scripts/dev.sh -p bluesky-feeds-consumer

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
	op run --env-file=.env.op -- cargo run --bin publish_feed $(FEED)

unpublish:
	op run --env-file=.env.op -- cargo run --bin unpublish_feed $(FEED)

check-image:
	cargo run -p bluesky-feeds-consumer --bin check_image $(IMAGE)
