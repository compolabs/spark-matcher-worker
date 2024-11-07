FROM rust:bullseye AS builder

WORKDIR /app
COPY . .

RUN apt-get update && apt-get install -y pkg-config libssl-dev
RUN rustup target add wasm32-unknown-unknown

RUN cargo build --release

FROM debian:bullseye-slim

WORKDIR /app

COPY --from=builder /app/target/release/spark-matcher-worker /app/

#COPY .env /app/.env

RUN apt-get update && apt-get install -y libssl1.1 ca-certificates && rm -rf /var/lib/apt/lists/*

CMD ["./spark-matcher-worker"]
