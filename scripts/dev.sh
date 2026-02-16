#!/bin/bash

# Load .env if exists
if [ -f .env ]; then
    set -a
    source .env
    set +a
fi

# Determine PRIVATELIST_URL
if [ -z "$PRIVATELIST_URL" ]; then
    echo "PRIVATELIST_URL is not set. Defaulting to local environment."
    export PRIVATELIST_URL="http://127.0.0.1:3000"
else
    echo "Using configured PRIVATELIST_URL: $PRIVATELIST_URL"
fi

# Run the server
mkdir -p data
RUST_LOG=info cargo run --bin bluesky-feeds
