# ビルドステージ
FROM rust:1.84-slim as builder

WORKDIR /build

# 依存関係のキャッシュ用にダミープロジェクトをビルド
COPY Cargo.toml Cargo.lock ./
COPY crates/shared/Cargo.toml ./crates/shared/Cargo.toml
COPY crates/helloworld/Cargo.toml ./crates/helloworld/Cargo.toml
RUN mkdir -p src crates/shared/src crates/helloworld/src && \
    echo "fn main() {}" > src/main.rs && \
    echo "pub fn dummy() {}" > crates/shared/src/lib.rs && \
    echo "pub fn dummy() {}" > crates/helloworld/src/lib.rs && \
    cargo build --release && \
    rm -rf src crates

# 実際のソースコードをコピーしてビルド
COPY . .
RUN cargo build --release

# ランタイムステージ
FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/bluesky-feeds /usr/local/bin/app

ENV PORT=3001
EXPOSE 3001

CMD ["/usr/local/bin/app"]
