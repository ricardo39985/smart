# SmartCindy

SmartCindy is planned as an Android/iOS assistant with streamed chat, durable history and personal memory, research, image capabilities, and account management. The Rust backend lives here; the separate Flutter client is in [`../smartcindy-app`](../smartcindy-app). See [architecture](docs/architecture.md) for selected direction.

## Current status

This repository contains the Rust backend for SmartCindy: native bearer authentication, paginated branch-aware chats, durable generation jobs, private images, owner-scoped personal memory/context, projects, content-free SSE invalidations, and session-bound native push registration. See the [API contract](docs/api.md), [OpenAPI schema](docs/openapi.json), [architecture decisions](docs/decisions/README.md), and [current verification and remaining gates](docs/status.md). Live provider delivery, real embedding-model quality/latency, and native memory/project/push acceptance are tracked separately. The sibling Flutter app has Chat, Your space and Create preview screens plus a verified Android shell/readiness journey; the API additions are not a claim that those native flows are implemented.

Read the contributor [instructions](AGENTS.md), [architecture](docs/architecture.md), [testing policy](docs/testing.md), [development workflow](docs/development.md), [status](docs/status.md), [decisions](docs/decisions/README.md), and [Danny build operations](docs/operations/danny-build.md).

The development API is running on Danny's existing Ubuntu-24.04 WSL2 and is reachable at `https://dev.smartcindy.app` through the existing Inspiron connector and SSH relay. This operator-controlled development runtime depends on both machines and awake Windows; it is not production or an unattended service. See [runtime operations](docs/operations/danny-development-api.md), [tunnel lifecycle](docs/operations/development-tunnel.md), and [verified status](docs/status.md).

Production targets **Debian on Lightsail with native services and no Docker deployment**. Docker remains useful for development builds and the PostgreSQL integration tests. See [ADR 0004](docs/decisions/0004-debian-native-deployment.md). No Debian instance has been provisioned or deployed.

## Development

Install the pinned Rust 1.96.0 toolchain with rustfmt and Clippy, plus Docker for real PostgreSQL integration tests. The requested [Blaze development workflow](docs/operations/blaze-development.md) is installed separately; its build compatibility limitations are recorded there. The resolved `Cargo.lock` is included. Run `sh scripts/check` (or its absolute path from elsewhere); this runs database/provider-simulator contracts, not live-provider or native acceptance.

Set protected environment values following `.env.example` (never commit real values). Run `cargo run --bin smartcindy -- migrate` with direct `MIGRATION_DATABASE_URL`, then `cargo run --bin smartcindy -- provision-user EMAIL` with password on protected stdin. `cargo run --bin smartcindy -- serve` uses the separate runtime `DATABASE_URL`, defaults to `127.0.0.1:8787`, and never implicitly migrates. Provider keys stay server-side. Missing keys disable new paid generation admissions. Complete the remaining release gates before deployment.

Rust follows Cargo conventions: library code is in `src/`, public behavioral integration tests belong in `tests/`, and examples or benchmarks are added only for real use cases. Run backend checks with `sh scripts/check`; Flutter checks are documented in [`../smartcindy-app`](../smartcindy-app/README.md). No hosted Actions or other CI service is configured. The first working slice is a signed-in user submitting a unique request, receiving streamed output, reopening durable history, and recovering that request after interruption.
