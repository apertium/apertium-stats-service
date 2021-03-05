FROM apertium/base
LABEL maintainer sushain@skc.name

RUN apt-get -qq update && \
    apt-get -qq install --no-install-recommends \
        curl \
        libssl-dev \
        libsqlite3-dev \
        subversion

RUN curl -s https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly-2021-03-04
ENV PATH="/root/.cargo/bin:${PATH}"

RUN cargo install diesel_cli --version 1.2.0 --no-default-features --features "sqlite"

# Build dependencies.
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src

# Force rebuild apertium-stats-service binary with real sources.
COPY . .
RUN touch src/main.rs
RUN cargo build --release

RUN diesel database setup

ENTRYPOINT ["cargo"]
CMD ["run", "--release"]
