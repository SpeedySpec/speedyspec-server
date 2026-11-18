# The speedyspec-server image: one static musl binary on an empty base.
#
#   docker build -t speedyspec-server .
#   docker build -t speedyspec-server:e2e --build-arg FEATURES=test-hooks .   (the end-to-end test's build)
FROM rust:1.99-alpine AS build
RUN apk add --no-cache build-base
WORKDIR /src
COPY . .
ARG FEATURES=""
RUN cargo build --release -p speedyspec-server ${FEATURES:+--features speedyspec-server/$FEATURES} \
    && cp target/release/speedyspec-server /speedyspec-server

FROM scratch
COPY --from=build /speedyspec-server /speedyspec-server
ENTRYPOINT ["/speedyspec-server"]
CMD ["run", "--config", "/etc/speedyspec/server.toml"]
