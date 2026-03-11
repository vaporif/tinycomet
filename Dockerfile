FROM golang:1.24-bookworm AS go-builder
RUN go install github.com/cometbft/cometbft/cmd/cometbft@v0.38.21

FROM rust:1.94-bookworm AS rust-builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    libgcc-s1 ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=go-builder /go/bin/cometbft /usr/local/bin/
COPY --from=rust-builder /src/target/release/tinycomet /usr/local/bin/
COPY --from=rust-builder /src/target/release/tinycomet-app /usr/local/bin/
COPY --from=rust-builder /src/target/release/tinycomet-proxy /usr/local/bin/
ENTRYPOINT ["tinycomet"]
