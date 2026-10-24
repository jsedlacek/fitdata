# Frontend: Vite build of the React dashboard.
FROM node:24-slim AS web
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY frontend ./
RUN npm run build

# Backend: cargo in the official Rust image. cargo-chef builds the
# dependencies in their own layer, which stays cached until Cargo.* changes.
FROM rust:1.99-slim-bookworm AS chef
RUN cargo install cargo-chef --version 0.1.78 --locked
WORKDIR /src

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src src
# Table schemas, compiled in (src/duck.rs).
COPY schema schema
RUN cargo build --release --locked

# Runtime: distroless (glibc, CA certs, tzdata).
FROM gcr.io/distroless/cc-debian12
COPY --from=build /src/target/release/fitdata /fitdata
COPY --from=web /web/dist /static
ENV STATIC_DIR=/static
ENTRYPOINT ["/fitdata"]
