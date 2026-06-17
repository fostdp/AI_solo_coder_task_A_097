FROM rust:1.77-alpine AS builder

RUN apk add --no-cache musl-dev pkgconf openssl-dev openssl-libs-static

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release 2>/dev/null || true

COPY src/ src/
RUN touch src/main.rs
RUN cargo build --release

FROM alpine:3.19 AS certs
RUN apk add --no-cache ca-certificates

FROM scratch

COPY --from=certs /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /app/target/release/guibiao-backend /guibiao-backend

EXPOSE 3000

ENTRYPOINT ["/guibiao-backend"]
