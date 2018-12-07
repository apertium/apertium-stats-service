FROM apertium/base
LABEL maintainer sushain@skc.name

RUN apt-get -qq update && \
    apt-get -qq install --no-install-recommends \
        curl \
        libssl-dev \
        libsqlite3-dev

RUN curl -s https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly-2018-12-06
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
