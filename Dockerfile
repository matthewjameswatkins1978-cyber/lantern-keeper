FROM rust:1.98.1-bookworm AS build

WORKDIR /src
COPY . .
RUN cargo build --locked --release -p lighting --bin lighting

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install --no-install-recommends --yes ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /home/lantern --shell /usr/sbin/nologin lantern

COPY --from=build /src/target/release/lighting /usr/local/bin/lighting

RUN mkdir -p /data/surrealkv \
    && chown -R lantern:lantern /data /home/lantern

USER lantern
WORKDIR /home/lantern

ENV LIGHTING_HOST=0.0.0.0 \
    LIGHTING_PORT=4317 \
    LIGHTING_STORAGE=embedded-surrealkv \
    LIGHTING_SURREAL_PATH=/data/surrealkv \
    WARDEN_PUBLIC_DEMO=1 \
    RUST_LOG=info

EXPOSE 4317
ENTRYPOINT ["/usr/local/bin/lighting"]
CMD ["serve"]
