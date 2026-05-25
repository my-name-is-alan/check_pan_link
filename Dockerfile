FROM rust:1.95-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --shell /usr/sbin/nologin appuser

WORKDIR /app

COPY --from=builder /app/target/release/check-pan-link /usr/local/bin/check-pan-link

ENV APP_HOST=0.0.0.0
ENV APP_PORT=8080
ENV CHECK_TIMEOUT_SECS=10
ENV RUST_LOG=info

EXPOSE 8080

USER appuser

ENTRYPOINT ["/usr/local/bin/check-pan-link"]
