# =============================================================================
# Crucible Backend — Multi-stage Dockerfile
# =============================================================================
#
# Stage 1: Build the Rust binary with cargo (builder)
# Stage 2: Create a minimal runtime image (runtime)
#
# This produces a small, secure production image (~100MB vs ~2GB for full Rust).
# =============================================================================

# ---------------------------------------------------------------------------
# Stage 1: Builder
# ---------------------------------------------------------------------------
ARG RUST_VERSION=1.78
FROM rust:${RUST_VERSION}-slim-bookworm AS builder

# Install build dependencies (for SQLx, OpenSSL, etc.)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    libpq-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache dependency compilation by copying manifests first
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo 'fn main() { println!("placeholder"); }' > src/main.rs
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src

# Copy the actual source code and build
COPY . .
RUN cargo build --release

# ---------------------------------------------------------------------------
# Stage 2: Runtime
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies only
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    libpq5 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user for security
RUN groupadd --gid 1001 crucible \
    && useradd --uid 1001 --gid crucible --shell /bin/false --create-home crucible

WORKDIR /app

# Copy the built binary from the builder stage
COPY --from=builder /app/target/release/crucible-backend /app/crucible-backend

# Copy migrations if they exist
COPY --from=builder /app/migrations /app/migrations

# Set ownership
RUN chown -R crucible:crucible /app

# Switch to non-root user
USER crucible

# Expose the application port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=15s --timeout=5s --start-period=60s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Run the application
ENTRYPOINT ["/app/crucible-backend"]
# Stage 1: Plan
FROM lukemathwalker/cargo-chef:latest-rust-1.78 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: Build
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release

# Stage 3: Runtime
FROM gcr.io/distroless/cc-debian12 AS runtime
WORKDIR /app
COPY --from=builder /app/target/release/backend /app/backend
COPY --from=builder /app/migrations /app/migrations
COPY --from=builder /app/.env.example /app/.env

EXPOSE 3000
ENTRYPOINT ["./backend"]
