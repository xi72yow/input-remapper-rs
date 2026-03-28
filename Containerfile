# --- base: Rust + System-Dependencies + Cargo-Tools ---
FROM docker.io/library/rust:1.94-bookworm AS base

RUN apt-get update && apt-get install -y --no-install-recommends \
    musl-tools kmod \
    && rustup target add x86_64-unknown-linux-musl \
    && cargo install --locked cargo-auditable cargo-audit cargo-deb \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
CMD ["bash"]
