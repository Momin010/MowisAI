# Multi-stage build for agentd
FROM rust:1.75-slim as builder

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY agentd/Cargo.toml ./agentd/
COPY agentd-protocol/Cargo.toml ./agentd-protocol/
COPY runtime/Cargo.toml ./runtime/
COPY mowis-gui/Cargo.toml ./mowis-gui/

# Copy source code
COPY agentd/src ./agentd/src
COPY agentd-protocol/src ./agentd-protocol/src
COPY runtime/src ./runtime/src

# Build release binaries
RUN cargo build --release --bin agentd --bin runtime

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -r -s /bin/false agentd

# Copy binaries
COPY --from=builder /app/target/release/agentd /usr/local/bin/
COPY --from=builder /app/target/release/runtime /usr/local/bin/

# Create directories
RUN mkdir -p /var/lib/agentd /var/log/agentd && \
    chown agentd:agentd /var/lib/agentd /var/log/agentd

# Expose socket path
VOLUME ["/tmp"]

# Default command
CMD ["agentd", "--help"]