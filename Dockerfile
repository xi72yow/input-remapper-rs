FROM rust:bookworm

RUN apt-get update && apt-get install -y \
    kmod \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

CMD ["bash"]
