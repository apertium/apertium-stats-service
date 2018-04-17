Apertium Stats Service
======================

[![Build Status](https://travis-ci.org/apertium/apertium-stats-service.png?branch=master)](https://travis-ci.org/apertium/apertium-stats-service)
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
    docker run --expose 8000 apertium-stats-service # or 80 for staging/prod

Development
-----------

Run `cargo fmt` to format code, `cargo clippy` to check for lint and
`cargo test` to run tests.

[1]: https://apertium.github.io/apertium-stats-service/
[2]: https://rocket.rs/guide/configuration
