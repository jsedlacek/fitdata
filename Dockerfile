# Build: cargo in the official Rust image; run: distroless (glibc, CA certs, tzdata).
FROM rust:1.99-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src src
COPY web web
RUN cargo build --release --locked

FROM gcr.io/distroless/cc-debian12
COPY --from=build /src/target/release/fitdata /fitdata
ENTRYPOINT ["/fitdata"]
