# Frontend: Vite build of the React dashboard.
FROM node:24-slim AS web
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY frontend ./
RUN npm run build

# Backend: cargo in the official Rust image.
FROM rust:1.99-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src src
RUN cargo build --release --locked

# Runtime: distroless (glibc, CA certs, tzdata).
FROM gcr.io/distroless/cc-debian12
COPY --from=build /src/target/release/fitdata /fitdata
COPY --from=web /web/dist /static
ENV STATIC_DIR=/static
ENTRYPOINT ["/fitdata"]
