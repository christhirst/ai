# ------------------------------------------------------------------------------
# Build Stage
# ------------------------------------------------------------------------------
FROM docker.io/library/rust:1-bookworm AS builder

WORKDIR /usr/src/app

# Copy manifest and source files
COPY Cargo.toml ./
COPY src ./src

# Build release binaries
RUN cargo build --release

# ------------------------------------------------------------------------------
# Runtime Stage
# ------------------------------------------------------------------------------
FROM docker.io/library/debian:bookworm-slim AS runtime

# Install CA certificates for HTTPS/TLS requests to Gemini API
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    tzdata \
    && rm -rf /var/lib/apt/lists/*

# Create non-root system user
RUN useradd -m -u 1000 -U appuser

WORKDIR /app

# Copy release binaries from builder stage
COPY --from=builder /usr/src/app/target/release/ai /usr/local/bin/ai
COPY --from=builder /usr/src/app/target/release/prompt /usr/local/bin/prompt
COPY --from=builder /usr/src/app/target/release/prompt_typed /usr/local/bin/prompt_typed

# Copy default example configuration
COPY config.toml.example /app/config.toml.example

# Set directory permissions for non-root user
RUN chown -R appuser:appuser /app

USER appuser

ENV RUST_LOG=info

# Default entrypoint runs the main binary (which executes the configured variant)
ENTRYPOINT ["/usr/local/bin/ai"]
