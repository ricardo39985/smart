# 0001: Start with one conventional Cargo package

Date: 2026-10-29

Status: Accepted for the repository foundation.

## Context

The product architecture has multiple runtime roles but no implemented product code. Creating many crates, empty service traits, and stub binaries now would add navigation and maintenance cost without enforcing useful behavior. The user requires clear instructions and minimal technical debt.

## Decision

Start with one `smartcindy` library package at the repository root using Cargo's standard layout. Pin Rust 1.96.0, the compiler available and verified for this foundation. Add thin executables under `src/bin/` with actual runnable slices. Add capability modules only when their behavior is implemented.

Keep migration and operations boundaries documented in their own folders. The earlier frontend-in-repository detail is superseded by [ADR 0002](0002-flutter-client-and-local-checks.md): the Flutter client is a separate sibling repository.

Split a package when a concrete build/runtime boundary warrants it. The private OpenVINO embedder is the expected first split. A Cargo workspace can then contain the root application package and embedder without moving public application contracts unnecessarily.

## Consequences

There is no runnable service yet and no invented health check. Scaffold compilation proves only compiler/layout compatibility. The module rules in `docs/architecture.md` still apply within the initial package. No empty-module tests, boilerplate behavior, speculative dependencies, or coverage target are introduced.

## Verification

Run formatting, compilation/lint, and documentation checks. Confirm that Cargo detects the package and that contributor links resolve. Add product tests when actual public behavior exists, following `docs/testing.md`.
