FROM rust:1-slim-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY app ./app
COPY replay ./replay
RUN cargo build --release --locked -p mergeable

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /build/target/release/mergeable /usr/local/bin/mergeable
ENV PORT=3000 \
    HOST=0.0.0.0
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/mergeable"]
