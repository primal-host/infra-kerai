FROM primal-host-infra-eridu:latest AS builder
WORKDIR /build

# Copy web crate and workspace Cargo.lock
COPY Cargo.lock ./web/
COPY web/ ./web/

# Build Rust binary (web crate only, no pgrx dependency)
RUN cd web && cargo build --release

# Build frontend
RUN cd web/frontend && npm install && npm run build

# Runtime
FROM ubuntu:24.04
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/web/target/release/kerai-web /usr/local/bin/
COPY --from=builder /build/web/frontend/dist /srv/static

ENV STATIC_DIR=/srv/static
EXPOSE 3000

CMD ["kerai-web"]
