Apertium Stats Service
======================

[![Build Status](https://github.com/github/apertium/apertium-stats-service/workflows/check.yml/badge.svg?branch=master)](https://github.com/apertium/apertium-stats-service/actions/workflows/check.yml?query=branch%3Amaster)
[![Coverage Status](https://coveralls.io/repos/github/apertium/apertium-stats-service/badge.svg?branch=master)](https://coveralls.io/github/apertium/apertium-stats-service?branch=master)

Stateful Rust web service that enables the efficient concurrent compilation
and distribution of statistics regarding Apertium packages via a RESTful API.

Usage
-----

See [`api.html`][1] for the Swagger UI representation of the OpenAPI 3.0 spec.

Running
-------

Build with `cargo build` and run with `cargo run`.

Edit `.env` to set environment parameters including those that control
[Rocket configuration][2].

Use `cargo build --release` to create production binaries or use the
provided `Dockerfile`:

    docker build -t apertium-stats-service .
    docker run -t -p 8000:8000 apertium-stats-service # or 80 for staging/prod

To persist data across restarts, use `docker-compose.yml` instead:

    docker-compose up --build

Development
-----------

Run `cargo fmt` to format code, `cargo clippy` to check for lint and
`cargo test` to run tests.

[1]: https://apertium.github.io/apertium-stats-service/
[2]: https://rocket.rs/guide/configuration
