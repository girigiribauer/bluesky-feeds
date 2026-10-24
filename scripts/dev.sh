#!/bin/bash

mkdir -p data
RUST_LOG=info cargo run "$@"
