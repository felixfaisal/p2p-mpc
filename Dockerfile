# Multi-stage build for optimized image size
FROM rust:1.92-slim as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src
COPY fixtures ./.mpc_cache
COPY config ./config

# Build the application in release mode
RUN cargo build --release --features development

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN useradd -m -u 1000 -s /bin/bash mpcuser

# Create app directory
WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/p2p-mpc /usr/local/bin/p2p-mpc

# Copy fixtures for cached Paillier primes
COPY --from=builder /app/.mpc_cache /app/.mpc_cache
COPY --from=builder /app/config /app/config

# Create directory for MPC cache and configs
RUN mkdir -p /app/configx && \
    chown -R mpcuser:mpcuser /app

# Switch to non-root user
USER mpcuser

# Expose ports
# 9000-9002: P2P network ports
# 8545-8547: RPC ports
# 9090-9092: Metrics ports
EXPOSE 9000 8545 9090

# Default environment variables
ENV RUST_LOG=info
ENV RUST_BACKTRACE=1

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD ["p2p-mpc", "--help"]

# Default command (can be overridden)
ENTRYPOINT ["p2p-mpc"]
CMD ["--help"]
