FROM rust:bookworm

RUN apt-get update && apt-get install -y \
    kmod \
    musl-tools \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app

CMD ["bash"]
