# syntax=docker/dockerfile:1

FROM rust:slim-bookworm AS chef
WORKDIR /build
RUN cargo install cargo-chef
RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev perl make && \
    rm -rf /var/lib/apt/lists/*

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS cacher
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

FROM chef AS builder
COPY . .
COPY --from=cacher /build/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo
RUN cargo build --release

FROM debian:trixie-slim AS runtime

RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/bluesky-feeds /usr/local/bin/app
WORKDIR /usr/local/bin

ENV PORT=3000
EXPOSE 3000

CMD ["/usr/local/bin/app"]
