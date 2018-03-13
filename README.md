Apertium Stats Service
======================

[![Build Status](https://travis-ci.org/apertium/apertium-stats-service.png?branch=master)](https://travis-ci.org/apertium/apertium-stats-service)

Stateful Rust web service that enables the efficient parallel compilation
and distribution of statistics regarding Apertium packages via a RESTful API.

Usage
-----
See `api.html` for the Swagger UI representation of the OpenAPI 3.0 spec.

Running
------

Build with `cargo build` and run with `cargo run`.
Edit `.env` to set environment parameters.

Development
----------

Run `cargo fmt` to format code.
