# Architecture and code boundaries

Status: selected product direction plus a verified durable backend and a running local development API/PostgreSQL on Inspiron. Actual evidence and outstanding gates live in [status.md](status.md); the concrete protocol is [api.md](api.md) and its decisions are [ADR 0003](decisions/0003-durable-chat-api.md).

Current development placement supersedes the earlier Danny runtime descriptions
below: API/PostgreSQL now run natively on Inspiron with the existing local tunnel,
and Danny remains the build workhorse. See [ADR 0011](decisions/0011-local-development-runtime.md)
and [local runtime operations](operations/local-development-api.md). Production
Debian/Lightsail direction is unchanged.

## Product boundary

SmartCindy is a native Android/iOS assistant, built from one shared Flutter codebase: streamed chat, saved history, person-level memory across chats, memory controls, research with citations, photo questions, image generation/editing, and account/subscription management. The selected customer offer is one USD 20 monthly subscription with fair use and no surprise overages. Native billing/distribution policy review is required before implementing the earlier Stripe proposal. Reminders are a planned extension; first-release inclusion is undecided.

## Runtime boundary

Production is one Debian Lightsail instance using native services, with no Docker deployment: Caddy, Rust web process, bounded Rust workers, a private embedding process, PgBouncer, PostgreSQL, and private attachment storage. Backups are encrypted and off-instance. The provisional 8-GB host and concurrency limits require measurement; they are not capacity guarantees. A single host is a single availability boundary.

| Responsibility | Selected direction |
| --- | --- |
| Native client | Flutter/Dart for Android and iOS in the separate smartcindy-app repository |
| HTTP and async work | Rust, Axum, Tokio; authenticated streaming |
| Outbound HTTP | Shared Reqwest clients, explicit features, Rustls |
| JSON | Serde and sonic-rs; serde_json where required by dependencies |
| Durable data | PostgreSQL 18/pgvector and SQLx migrations; first-slice private images in quota-bounded bytea; full-text search remains planned |
| Connection management | SQLx pools through PgBouncer; direct migration connection |
| Context and memory machinery | PostgreSQL-owned context assembly and bounded background extraction; overlapping Rig/rig-memory/mem0-rs engines remain deferred after adapter review (ADR 0007) |
| Local embeddings | Bekko a8m, 384 dimensions, OpenVINO in a private process |
| Hosted AI | Previously selected DeepSeek Flash, Search/Fetch, hosted FLUX.2 Klein fast/distilled endpoint |
| Accounts and billing | Local signup and Argon2id passwords; Google/Apple verified identities with explicit linking; hashed opaque native sessions in PostgreSQL; email verification/recovery and billing remain planned |
| Jobs and reminders | Durable PostgreSQL jobs, leases, bounded Rust workers; native push (FCM/APNs direction), with platform behavior to define before reminder delivery |
| Operations | Debian, native services with systemd, Caddy, pgBackRest, S3; Docker permitted for development/tests only; no GitHub Actions |

Provider names are architectural selections inherited from the planning chat. Before adding each integration, inspect the current official API contract, resolve exact endpoint/model/version, and pin the compatible dependency. Do not infer implemented capability from this table.

Production and development packaging follow [ADR 0004](decisions/0004-debian-native-deployment.md). Keep PostgreSQL/pgvector Testcontainers for development. Native Debian release compatibility must be verified separately; no deployment has occurred.

## Client and development boundary

The Rust backend lives at `/home/rick/Documents/Code/smartcindy`; the separate Flutter repository is `/home/rick/Documents/Code/smartcindy-app`. Android and iOS share Dart code; platform glue stays minimal. No browser or desktop target is selected. Each repository has its own lockfile, contributor instructions, and manual check script. No GitHub Actions or replacement hosted CI is configured.

The backend owns the actual API contract. Define schemas, streaming events, errors, authentication/session expiry and revocation, request IDs, cancellation, and reconnect before implementing a consumer. Generate Dart types from that real contract rather than maintaining parallel definitions. Secure device storage replaces assumptions about browser cookie persistence; provider secrets remain exclusively server-side. Do not create speculative contract code or duplicate server business rules in the client.

Development hostname: `https://dev.smartcindy.app`, routed through the existing Inspiron Cloudflare user service directly to the local native API at `127.0.0.1:8787`. PostgreSQL/pgvector runs locally in a private native userspace. Direct/public readiness and authenticated API behavior passed after the 2026-10-29 move. The runtime depends on this machine, network and user services; it is not unattended availability. Danny remains the workhorse with its existing bounded build restrictions. See [runtime operations](operations/local-development-api.md), [tunnel operations](operations/development-tunnel.md), and [move verification](verification/2026-10-29-local-runtime-move.md).

Native notifications use an FCM/APNs integration when implemented. Service workers and Web Push are browser mechanisms and are not the selected native delivery path. Native user journeys use Flutter integration_test on devices/emulators; Playwright MCP remains for actual browser surfaces such as a hosted portal. iOS build and device/simulator acceptance require macOS with Xcode.

## Repository layout

Follow [Cargo package conventions](https://doc.rust-lang.org/cargo/guide/project-layout.html). See [ADR 0001](decisions/0001-small-cargo-foundation.md) for the small Rust package and [ADR 0002](decisions/0002-flutter-client-and-local-checks.md) for the separate Flutter client and manual checks.

| Path | Authority / responsibility |
| --- | --- |
| `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` | Package, resolved dependencies, compiler pin |
| `src/lib.rs` | Library root; expose only deliberate public contracts |
| `src/<capability>.rs` or `src/<capability>/` | Rules and use cases grouped by behavior as implemented |
| `src/bin/` | Thin web/worker entry points when runnable slices exist |
| `tests/` | Tests through public Rust/HTTP interfaces; shared support under `tests/support/` when needed |
| Sibling `../smartcindy-app/` | Flutter client; shared Dart in lib/, Android/iOS platform glue, native integration journeys |
| `migrations/` | Ordered SQLx migrations when durable storage is introduced |
| `ops/` | Reviewed deployment/service configuration when needed |
| `scripts/` | Small checked development entry points |
| `docs/` | Architecture, contributor policies, decisions, current status, operations evidence |

The backend now has concrete auth, chat/history, attachment, provider and worker modules, a thin CLI, ordered migrations and verified public HTTP scenarios. The Flutter app supplies a three-view preview shell and a real development health check; its Android shell journey passed, while authenticated native chat and live-provider acceptance remain pending. Document future capabilities rather than creating empty modules and pretend services. Create `examples/` and `benches/` only for an actual example or measurement.

## Dependency rules

Entry points assemble dependencies and translate transport. Capability modules implement use cases and rules. Storage and provider code handle concrete IO. Core rules must not import Axum request types or rely on SQLx rows as business types. Start with concrete implementations; introduce narrow traits only for genuine replaceable IO boundaries, not for mocking every function. Avoid a shared `utils` dumping ground.

Keep modules private by default. Expose a use-case API when an external caller needs it. Tests are consumers of that API; do not widen visibility just for tests. Split a crate only when independent native dependencies, deployment, or enforced dependency direction justify it. The OpenVINO embedder is the expected first independent package so native inference does not burden the web executable.

## Invariants to preserve as features arrive

- PostgreSQL is authoritative for users, sessions, ordered messages, summaries, memory versions, jobs, entitlements, and usage. Library autosaves must not create another transcript authority.
- Validate ownership before every read, write, attachment access, and retrieval. Personal memory is person-scoped; conversation context is chat-scoped.
- Accept a unique client request before generation. Persist the final answer, usage settlement, status, and pending jobs transactionally. Reconnect recovers that request; unknown provider outcomes require reconciliation.
- Keep transactions brief. Release connections before external calls, local inference, and streams. Coordinate conflicting turns with leases and version checks rather than a long-held lock.
- Claim jobs with short transactions and fenced leases. Stale workers cannot overwrite newer state. Retries cannot duplicate accepted operations or silently repeat paid submissions.
- Retain source references and versions for summaries/memory. Corrections affect the next turn; deletion/disablement invalidates retrieval and stale publication.
- The selected mem0-rs port has an auxiliary SQLite history store. Before integration, inspect its pinned source and implement scoped purge, bounded access, backup, and reconciliation; do not assume cross-store atomicity.
- Reserve bounded usage before paid work and settle actual cost afterward. Signed webhook state controls entitlements; a checkout redirect does not.
- Keep queues, tool loops, uploads, inference, retries, and provider concurrency bounded. Live chat gets capacity ahead of background work.
- Keep content and secrets out of routine logs. Back up data and private blobs consistently; demonstrate rollback and restore before launch.

## Changes and source precedence

User instructions and explicit acceptance criteria govern scope. Accepted ADRs govern structural choices. This file governs the selected architecture; `docs/status.md` states what works now. Docs and code must agree in the same change. A conflict is resolved and documented, never silently interpreted as permission to invent behavior.

Planning source: [Personal AI Assistant Development Pipeline](https://chatgpt.com/space/page_b97adf45c0a88191b3ca5ad3b713a018), plus later planning-chat decisions for PgBouncer, sonic-rs, explicit Reqwest features, and reminders. The user's Flutter and no-Actions decisions in ADR 0002 supersede the older Page's web frontend and delivery choices. This repository is the backend engineering source of truth going forward; changes here do not automatically update that Page.

## Native account authentication

`src/auth.rs` owns account/password/session operations; `src/identity.rs` owns bounded Google/Apple protocol handling and proof-bound native exchange. PostgreSQL remains the identity authority. The mobile client uses the system authentication browser and secure device storage; provider secrets remain server-side. Matching email never automatically links accounts. See [ADR 0005](decisions/0005-native-account-authentication.md), [API contract](api.md) and [provider operations](operations/authentication.md).

## Memory and context implementation boundary

[ADR 0007](decisions/0007-memory-context.md) defines the implemented source layout:
`memory` owns user controls and epochs, `context` assembles stable personality,
recent branch history and hybrid recall, and `memory_worker` indexes source text
and performs batched extraction/summarization. Explicit controls and inferred
corrections fence stale publication. Source transcripts remain authoritative.
The private Python/OpenVINO process isolates native inference from Rust; requests
use literal loopback, bounded concurrency and a pinned model revision. Keyword
retrieval remains available when local inference is disabled or unavailable.
The integration is written, but real PostgreSQL journeys, embedding quality and
live-provider compatibility must pass before these are production guarantees.

Projects organize chats without changing their ownership or memory scope. Deleting
a project detaches its chats; deleting a chat removes its derived memory/context.
