# syntax=docker/dockerfile:1

FROM rust:1.99.0-slim-bookworm AS chef
WORKDIR /build
RUN cargo install cargo-chef --version 0.1.78 --locked
RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev perl make cmake g++ git && \
    rm -rf /var/lib/apt/lists/*
COPY scripts/build-ncnn.sh /tmp/build-ncnn.sh
RUN sh /tmp/build-ncnn.sh /opt/ncnn
ENV NCNN_DIR=/opt/ncnn

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS cacher
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

FROM chef AS builder-consumer
COPY . .
COPY --from=cacher /build/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo
RUN cargo build --release -p bluesky-feeds-consumer --bin consumer

FROM chef AS builder-server
COPY . .
COPY --from=cacher /build/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo
RUN cargo build --release -p bluesky-feeds --bin bluesky-feeds

FROM debian:trixie-slim AS runtime

RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /usr/local/bin

FROM runtime AS consumer

COPY --from=builder-consumer /build/target/release/consumer /usr/local/bin/app
COPY crates/silentviews/consumer/models/*.ncnn.param crates/silentviews/consumer/models/*.ncnn.bin crates/silentviews/consumer/models/clip_meta.json crates/silentviews/consumer/models/clip_text_embeds.json /usr/local/share/silentviews/models/
COPY THIRD_PARTY_NOTICES /usr/local/share/silentviews/THIRD_PARTY_NOTICES
ENV SILENTVIEWS_MODELS_DIR=/usr/local/share/silentviews/models

CMD ["/usr/local/bin/app"]

FROM runtime AS server

COPY --from=builder-server /build/target/release/bluesky-feeds /usr/local/bin/app

ENV PORT=3000
EXPOSE 3000

CMD ["/usr/local/bin/app"]
