# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/KarpelesLab/fullrust/compare/fullrust-v0.1.1...fullrust-v0.2.0) - 2026-08-03

### Other

- Use purestd as the standard library; slim fullrust to the runtime
- Real per-thread TLS (thread_local! via #[thread_local])

## [0.1.1](https://github.com/KarpelesLab/fullrust/compare/fullrust-v0.1.0...fullrust-v0.1.1) - 2026-06-05

### Other

- feature-gate binary-policy symbols behind `rt` (default on)
