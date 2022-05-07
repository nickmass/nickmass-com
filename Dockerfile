FROM docker.io/debian:stable-slim AS builder

RUN apt-get update && apt-get install curl openssl libssl-dev make gcc pkg-config -y
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --default-host x86_64-unknown-linux-gnu
ENV PATH="/root/.cargo/bin:${PATH}"
RUN cargo install cargo-make

COPY . /build

WORKDIR /build
RUN cargo make

FROM docker.io/debian:stable-slim
RUN apt-get -y update && apt-get -y install ca-certificates openssl

WORKDIR /app/
COPY --from=builder /build/dist/ /app

EXPOSE 4000
VOLUME /app/config

ENTRYPOINT ["/app/nickmass-com", "--config", "/app/config/config.toml"]
