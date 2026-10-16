# ==============================================================================
# Stage 1: Build stage
# ==============================================================================
FROM rust:1-slim-bookworm AS builder

WORKDIR /usr/src/finchspeak

# Install build dependencies for OpenSSL and native compilation
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace configuration and manifests
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY app ./app

# Build only the server in release mode
# finch-server is isolated and does not depend on finch-audio or finch-screen
RUN cargo build --release -p finch-server

# ==============================================================================
# Stage 2: Runtime stage
# ==============================================================================
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies: TLS certificates and OpenSSL runtime library
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create unprivileged user and group
RUN groupadd -g 10001 finch && \
    useradd -u 10001 -g finch -s /bin/sh -m finch

# Set up working directory and application data directory
WORKDIR /app
RUN mkdir -p /app/data && chown -R finch:finch /app

# Copy compiled binary from builder stage
COPY --from=builder --chown=finch:finch /usr/src/finchspeak/target/release/finch-server /usr/local/bin/finch-server

# Switch to non-privileged user
USER finch:finch

# Expose QUIC SFU (UDP) and REST API (TCP)
EXPOSE 4433/udp
EXPOSE 8443/tcp

# The server answers /health (database reachable) without a login; its certificate is self-signed.
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -kfsS https://localhost:8443/health || exit 1

# Server entrypoint
ENTRYPOINT ["/usr/local/bin/finch-server"]
