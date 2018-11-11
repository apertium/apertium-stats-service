FROM debian:jessie-slim
LABEL maintainer sushain@skc.name

RUN apt-get -qq update && \
    apt-get -qq install --no-install-recommends \
        ca-certificates \
        curl \
        gcc \
        libc-dev \
        libsqlite3-dev \
        libssl-dev \
        make \
        pkg-config \
        subversion
RUN curl -s https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly-2018-07-16
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src
COPY . .
RUN cargo build --release

RUN cargo install diesel_cli --version 1.2.0 --no-default-features --features "sqlite"
RUN diesel database setup

ENTRYPOINT ["cargo"]
CMD ["run"]
