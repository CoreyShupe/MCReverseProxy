FROM rust:1.65-alpine AS chef

ARG CHEF_TAG=0.1.50

RUN ((cat /etc/os-release | grep ID | grep alpine) && apk add --no-cache musl-dev || true) \
    && cargo install cargo-chef --locked --version $CHEF_TAG \
    && rm -rf $CARGO_HOME/registry/

WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

COPY --from=planner /app/recipe.json recipe.json

ARG BUILD_PROFILE
ARG BUILD_PATH=$BUILD_PROFILE

RUN cargo chef cook --profile $BUILD_PROFILE --recipe-path recipe.json

COPY . .

RUN cargo build --profile $BUILD_PROFILE
RUN chmod +x target/$BUILD_PATH/mc_reverse_proxy

FROM alpine AS runtime

RUN apk add --update \
    su-exec \
    tini \
    curl \
    vim \
    openssl \
    bash

WORKDIR /app

ARG BUILD_PROFILE
ARG BUILD_PATH=$BUILD_PROFILE

COPY --from=builder /app/target/$BUILD_PATH/mc_reverse_proxy /app/executable

RUN adduser -D app -s /sbin/nologin
RUN chown -R app:app /app/

ENTRYPOINT ["/sbin/tini", "--"]
CMD ["su-exec", "app", "/app/executable"]