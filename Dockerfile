# ------------------------------------------------------------------------------
# Build Stage
# ------------------------------------------------------------------------------
FROM docker.io/library/rust:1-bookworm AS builder

WORKDIR /usr/src/app

# Copy manifests, build script, proto definitions, and source files
COPY Cargo.toml Cargo.lock* build.rs ./
COPY proto ./proto
COPY src ./src

# Build all release binaries
RUN cargo build --release --bins

# ------------------------------------------------------------------------------
# Runtime Stage
# ------------------------------------------------------------------------------
FROM gcr.io/distroless/cc-debian12 AS runtime

WORKDIR /app

# Copy release binaries from builder stage
COPY --from=builder /usr/src/app/target/release/grpc_server /usr/local/bin/grpc_server
COPY --from=builder /usr/src/app/target/release/grpc_client /usr/local/bin/grpc_client
COPY --from=builder /usr/src/app/target/release/ai /usr/local/bin/ai
COPY --from=builder /usr/src/app/target/release/prompt /usr/local/bin/prompt
COPY --from=builder /usr/src/app/target/release/prompt_typed /usr/local/bin/prompt_typed

# Copy default example configuration
COPY --chown=nonroot:nonroot config.toml.example /app/config.toml.example

USER nonroot:nonroot

ENV RUST_LOG=info
ENV GRPC_HOST=0.0.0.0
ENV GRPC_PORT=50051

# Expose Tonic gRPC server port
EXPOSE 50051

# Default entrypoint starts the Tonic gRPC server
ENTRYPOINT ["/usr/local/bin/grpc_server"]
