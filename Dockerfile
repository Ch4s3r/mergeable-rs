FROM rust:1-slim-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY app ./app
RUN cargo build --release --locked -p mergeable

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
COPY --from=build /build/target/release/mergeable /usr/local/bin/mergeable
ENV PORT=3000 \
    HOST=0.0.0.0
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/mergeable"]
