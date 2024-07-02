FROM docker.io/rust:latest AS builder

RUN cargo install cargo-make
RUN apt-get update && apt-get install -y cmake

WORKDIR /build
COPY . /build

RUN cargo make

FROM gcr.io/distroless/cc-debian12

WORKDIR /app
COPY --from=builder /build/dist /app

EXPOSE 4000
VOLUME /app/config

ENTRYPOINT ["/app/nickmass-com" ]
