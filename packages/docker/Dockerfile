# syntax=docker/dockerfile:1
#
# Multi-arch Docker image for mp3rgain.
#
# Built natively per arch (linux/amd64, linux/arm64) by .github/workflows/release.yml.
# rust:alpine produces a fully static musl binary, so the runtime stage is `scratch`.
#
# Usage (drop-in mp3gain replacement for cron / Plex maintenance):
#   docker run --rm -v /path/to/music:/music \
#     ghcr.io/m-igashi/mp3rgain:latest -r -R /music

FROM rust:1-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --features replaygain --locked \
    && strip target/release/mp3rgain

FROM scratch
COPY --from=builder /build/target/release/mp3rgain /usr/local/bin/mp3rgain
ENTRYPOINT ["/usr/local/bin/mp3rgain"]
