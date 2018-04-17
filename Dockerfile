FROM debian:jessie-slim
LABEL maintainer sushain@skc.name

RUN apt-get -qq update && \
    apt-get -qq install --no-install-recommends \
        ca-certificates \
        curl \
        gcc \
        libc-dev \
        libssl-dev \
        make \
        pkg-config \
        sqlite
RUN curl -s https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src
COPY . .
RUN cargo build --release

RUN cargo install diesel_cli --no-default-features --features "sqlite"
RUN diesel database setup

CMD ["cargo", "run"]
