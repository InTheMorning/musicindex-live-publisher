# The image of the publisher (ADR 0019). It holds musicindex-live-publisher
# only. mixxx-now-playing reads the Mixxx database and the MIDI device of the
# host, so it runs on the host and writes the drop files into /watch.
# ../stophammer/Dockerfile is the model of this file.

FROM rust:alpine AS chef

# openssl-libs-static links OpenSSL into the binary for native-tls. The
# runtime image then needs no OpenSSL package.
RUN apk add --no-cache \
    build-base \
    musl-dev \
    openssl-dev \
    openssl-libs-static \
    pkgconf \
 && cargo install cargo-chef

ENV OPENSSL_STATIC=1

WORKDIR /build

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --package musicindex-live-publisher --recipe-path recipe.json

COPY . .
# build.rs gives these two values to --version (musicindex ADR 0083). The
# context has no .git folder, so a build without them has no git revision.
# They come after `cargo chef cook`, so they do not spoil its cache.
ARG MUSICINDEX_LIVE_PUBLISHER_GIT_REVISION
ARG MUSICINDEX_LIVE_PUBLISHER_BUILT_AT
ENV MUSICINDEX_LIVE_PUBLISHER_GIT_REVISION=$MUSICINDEX_LIVE_PUBLISHER_GIT_REVISION \
    MUSICINDEX_LIVE_PUBLISHER_BUILT_AT=$MUSICINDEX_LIVE_PUBLISHER_BUILT_AT
RUN cargo build --release --locked --package musicindex-live-publisher --bin musicindex-live-publisher

# ── Runtime ────────────────────────────────────────────────────────────────────

FROM alpine:3.24@sha256:294b683cb724975bec92580e1e685676bd4b50bda910ddb8c51d4cabeaec77e6 AS publisher

RUN apk add --no-cache ca-certificates

COPY --from=builder /build/target/release/musicindex-live-publisher /usr/local/bin/musicindex-live-publisher

# The configuration that the setup helper writes holds absolute host paths:
# token_file, watch_dir and display_dir. Thus the operator mounts each host
# folder at the same path, starts the container with the host user ID, and
# gives the arguments of the systemd unit. The instance check of ADR 0016
# then reads the other instances, as on the host. ADR 0019 §2 gives the
# command.
ENTRYPOINT ["musicindex-live-publisher"]
