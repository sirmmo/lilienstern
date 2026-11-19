# The game server (crates/lilienstern-server), built from the repository's root:
#   docker build -f deploy/server/Dockerfile -t lilienstern-server .
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo build --release --locked -p lilienstern-server

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 lil && mkdir /data && chown lil /data
COPY --from=build /src/target/release/lilienstern-server /usr/local/bin/lilienstern-server
USER lil
ENV LIL_ADDR=0.0.0.0:8080 LIL_DATA=/data
EXPOSE 8080
VOLUME /data
CMD ["lilienstern-server"]
