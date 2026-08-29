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
FROM gcr.io/distroless/cc-debian12 AS runtime

WORKDIR /app

# Copy release binaries from builder stage
COPY --from=builder /usr/src/app/target/release/ai /usr/local/bin/ai
COPY --from=builder /usr/src/app/target/release/prompt /usr/local/bin/prompt
COPY --from=builder /usr/src/app/target/release/prompt_typed /usr/local/bin/prompt_typed

# Copy default example configuration
COPY --chown=nonroot:nonroot config.toml.example /app/config.toml.example

USER nonroot:nonroot

ENV RUST_LOG=info

# Default entrypoint runs the main binary (which executes the configured variant)
ENTRYPOINT ["/usr/local/bin/ai"]
