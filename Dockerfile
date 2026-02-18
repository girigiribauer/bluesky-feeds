# syntax=docker/dockerfile:1

# Base stage for common tools
FROM rust:slim-bookworm AS chef
WORKDIR /build
RUN cargo install cargo-chef
RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev perl make && \
    rm -rf /var/lib/apt/lists/*

# Stage 1: Planner
# Computes a lock-like file for the project (recipe.json)
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: Cacher
# Builds dependencies based on the recipe
FROM chef AS cacher
COPY --from=planner /build/recipe.json recipe.json
# Build dependencies - this is the cached layer
RUN cargo chef cook --release --recipe-path recipe.json

# Stage 3: Builder
# Builds the actual application
FROM chef AS builder
COPY . .
# Copy over the cached dependencies
COPY --from=cacher /build/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo
RUN cargo build --release

# Stage 4: WebUI Builder
FROM node:20-slim AS webui-builder
WORKDIR /build/webui
COPY webui/package*.json ./
RUN npm install
COPY webui/ .
RUN npm run build

# Stage 5: Runtime
FROM debian:trixie-slim AS runtime

RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/bluesky-feeds /usr/local/bin/app
COPY --from=webui-builder /build/webui/dist /usr/local/bin/webui/dist
WORKDIR /usr/local/bin

ENV PORT=3000
EXPOSE 3000

CMD ["/usr/local/bin/app"]
