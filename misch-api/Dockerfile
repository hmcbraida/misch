# syntax=docker/dockerfile:1.7
FROM rust:1.88-bookworm AS builder

WORKDIR /src
RUN --mount=type=bind,source=.,target=/src,rw \
	--mount=type=cache,target=/usr/local/cargo/registry \
	--mount=type=cache,target=/usr/local/cargo/git \
	--mount=type=cache,target=/src/target \
	cargo build --release -p misch-api && cp /src/target/release/misch-api /tmp/misch-api

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
	&& apt-get install -y --no-install-recommends ca-certificates \
	&& rm -rf /var/lib/apt/lists/*

COPY --from=builder /tmp/misch-api /usr/local/bin/misch-api

EXPOSE 8000
ENV ROCKET_ADDRESS=0.0.0.0
ENV ROCKET_PORT=8000
ENV ROCKET_LOG_LEVEL=normal

CMD ["misch-api"]
