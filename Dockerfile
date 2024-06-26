FROM lukemathwalker/cargo-chef:latest AS chef
WORKDIR app
## Planner stage: Cache dependencies
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

## Builder stage: Build binary
FROM chef AS builder 
COPY --from=planner /app/recipe.json recipe.json
# Install system dependencies
RUN apt-get update && apt-get -y upgrade && apt-get install -y libclang-dev pkg-config
# Build dependencies - this is the caching Docker layer
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release

## Runtime stage: Copy binary to new image and run 
FROM ubuntu:24.04 AS runtime
WORKDIR app
COPY --from=builder /app/target/release/artemis ./
# Install openssl and ca-certificates
RUN apt-get update && apt install -y libsqlite3-dev && apt install -y openssl && apt install -y ca-certificates
ENTRYPOINT ["./artemis", "--config-path", "/opt/liquidation-bot/config.json"]
CMD ["--private-key"]