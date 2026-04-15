# Ciel Verdict Layer — Multi-stage Rust build
# See spec Section 16.2 for deployment architecture.

FROM rust:1.78-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY src/ src/
COPY proto/ proto/
RUN apt-get update && apt-get install -y protobuf-compiler pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/ciel /usr/local/bin/ciel
EXPOSE 8080 50051
CMD ["ciel"]
