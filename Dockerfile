# Two stages so the shipped image carries no toolchain. SQLite is compiled into
# the binary (rusqlite's `bundled` feature) and the timezone database into
# chrono-tz, so the runtime layer needs nothing but libc.

FROM rust:1-trixie AS build
WORKDIR /src

# Dependencies first, against a stub main, so editing the app does not rebuild
# the crates underneath it. Needs the full rust image rather than the slim one:
# `bundled` SQLite is C and wants a compiler.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
 && echo 'fn main() {}' > src/main.rs \
 && cargo build --release \
 && rm -rf src

# The migrations are `include_str!`'d into the binary, so they are build inputs
# rather than runtime files and never appear in the second stage.
COPY migrations ./migrations
COPY src ./src
# The stub's artefacts are newer than the real sources we just copied, so cargo
# would consider the binary up to date and ship the stub.
RUN touch src/main.rs && cargo build --release


FROM debian:trixie-slim

LABEL org.opencontainers.image.title="Trusted Planner" \
      org.opencontainers.image.description="A self-hosted week and four-week planner for a home network" \
      org.opencontainers.image.source="https://github.com/pnahratow/trusted-planner" \
      org.opencontainers.image.licenses="AGPL-3.0-or-later"

# curl is only here for HEALTHCHECK below; it is also the thing you reach for
# first when the app is unreachable from a NAS shell.
RUN apt-get update \
 && apt-get install -y --no-install-recommends curl \
 && rm -rf /var/lib/apt/lists/*

# 568:568 is TrueNAS SCALE's `apps:apps`, the UID it runs custom apps as. Any
# other UID works too — nothing here reads /etc/passwd — as long as the host
# directory mounted at /data is writable by it.
RUN groupadd -g 568 -o apps && useradd -u 568 -g 568 -o -M -d /data apps

WORKDIR /app
COPY --from=build /src/target/release/trusted-planner /usr/local/bin/trusted-planner
COPY templates ./templates
COPY static ./static
COPY locales ./locales
# The image is a distribution of this software and everything linked into it,
# so the licence and the attribution travel with it.
COPY LICENSE THIRD-PARTY.md ./

# Templates and translations are read from disk on every render, so
# bind-mounting over either directory lets you edit the markup or reword the
# German on the NAS without rebuilding anything.
ENV PLANNER_TEMPLATE_DIR=/app/templates \
    PLANNER_STATIC_DIR=/app/static \
    PLANNER_LOCALE_DIR=/app/locales \
    PLANNER_DATA_DIR=/data \
    PLANNER_PORT=8080

# Created here so `docker run` with no mount still works; a bind mount replaces
# it and brings the host's ownership with it.
RUN mkdir -p /data && chown 568:568 /data
VOLUME /data

USER 568:568
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD curl -fsS "http://127.0.0.1:${PLANNER_PORT}/healthz" || exit 1

CMD ["trusted-planner"]
