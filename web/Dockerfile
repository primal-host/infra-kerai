FROM infra-eridu:latest AS builder
WORKDIR /build

# Workspace root — Cargo needs this to resolve members
COPY Cargo.toml Cargo.lock ./

# Copy kerai-cli crate (web depends on its lang module)
COPY kerai/ ./kerai/

# Stub postgres crate — workspace member but not built
RUN mkdir -p postgres/src && \
    echo '[package]\nname = "kerai"\nversion = "0.1.0"\nedition = "2021"\n\n[lib]\ncrate-type = ["cdylib"]' > postgres/Cargo.toml && \
    echo '' > postgres/src/lib.rs

# Copy web crate
COPY web/ ./web/

# Build Rust binary (web crate only)
RUN cargo build --release -p kerai-web

# Runtime
FROM ubuntu:24.04
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/kerai-web /usr/local/bin/

EXPOSE 62830

CMD ["kerai-web"]
