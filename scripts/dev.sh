#!/bin/bash

if [ -f .env ]; then
    set -a
    source .env
    set +a
fi

mkdir -p data
RUST_LOG=info cargo run --bin bluesky-feeds
