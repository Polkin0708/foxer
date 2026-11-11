FROM rust:1.85.1-bookworm@sha256:e51d0265072d2d9d5d320f6a44dde6b9ef13653b035098febd68cce8fa7c0bc4 AS rust-base

FROM rust-base AS dev
RUN rustup component add rustfmt clippy
WORKDIR /workspace

FROM rust-base AS source
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY core core
COPY bot/Cargo.toml bot/Cargo.toml
COPY bot/src bot/src
COPY backtest/Cargo.toml backtest/Cargo.toml
COPY backtest/src backtest/src

FROM source AS bot-build
RUN cargo build --locked --release -p forex-bot

FROM source AS backtest-build
RUN cargo build --locked --release -p forex-backtest

FROM scratch AS release
COPY --from=bot-build /build/target/release/forex-bot /forex-bot

FROM python:3.12.8-slim-bookworm@sha256:2199a62885a12290dc9c5be3ca0681d367576ab7bf037da120e564723292a2f0 AS backtest
RUN apt-get update && apt-get install -y --no-install-recommends fonts-dejavu-core=2.37-6 poppler-utils=22.12.0-2+deb12u3 time=1.9-0.2 && rm -rf /var/lib/apt/lists/*
COPY backtest/report/requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir -r /tmp/requirements.txt
WORKDIR /app
COPY --from=backtest-build /build/target/release/forex-backtest /usr/local/bin/forex-backtest
COPY backtest/report backtest/report
COPY backtest/data-sources.json backtest/data-sources.json
ENV MPLCONFIGDIR=/tmp/matplotlib PYTHONDONTWRITEBYTECODE=1
ENTRYPOINT ["forex-backtest"]
