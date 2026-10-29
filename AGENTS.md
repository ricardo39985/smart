# SmartCindy contributor instructions

These instructions apply to humans and coding agents throughout this repository.

## Read before changing code

1. Read [README.md](README.md), [architecture](docs/architecture.md), and [development workflow](docs/development.md).
2. Read [testing policy](docs/testing.md) before adding, changing, deleting, or running tests.
3. Check [current status](docs/status.md) and relevant [decisions](docs/decisions/README.md). Planned work is not implemented behavior.
4. For remote work, read [Danny build operations](docs/operations/danny-build.md). Keep credentials and host control outside coding agents and build jobs.
5. For hostname/tunnel work, read [development tunnel operations](docs/operations/development-tunnel.md). Follow its recorded authorization and lifecycle; preserve other tunnels.

## Implement the smallest complete behavior

- Preserve public behavior during refactoring. Organize code around capabilities, not one class/function per file.
- Follow Cargo's standard `src`, `tests`, `examples`, and `benches` layout. Start with one Rust package; create another only for an actual runtime/dependency boundary.
- Keep entry points thin. Business rules belong in the library. Prefer private modules and `pub(crate)`; a public API is an intentional contract.
- This is the Rust backend repository; the Flutter Android/iOS client is the separate sibling `../smartcindy-app`. No GitHub Actions. Use repository-local checks and reviewed operator workflows.
- Production targets native Debian services on Lightsail, without Docker or Compose. Docker is permitted for development/builds and Testcontainers. Follow [ADR 0004](docs/decisions/0004-debian-native-deployment.md); do not treat development containers as deployed artifacts.
- Do not add empty service layers, generic repositories, speculative traits, placeholder endpoints, or dependencies for future work. Introduce an abstraction when an actual boundary requires it.
- PostgreSQL owns application state. Never hold database locks/connections across provider calls, local inference, or streaming.
- Enforce ownership, durable request IDs, version checks, and bounded work where the behavior requires them. Never return success for an unimplemented operation.
- Do not silently change product scope, provider selections, security boundaries, or public contracts. Record architectural changes in an ADR and update affected docs in the same change. Research routine implementation details independently.

## Tests and verification

- Assert outcomes through public APIs and real user flows. No tests of private method calls, internal layout, or read-call order.
- Prefer real dependencies. Database integration tests use Testcontainers with PostgreSQL/pgvector, never SQLite as a substitute.
- Reproduce a bug with a failing behavioral test before fixing it. Verify the failure is the intended regression, then verify the fix with the same test.
- Native journeys use Flutter integration_test on Android/iOS devices or emulators. Playwright MCP is only for actual browser surfaces (for example hosted checkout), never native acceptance. No hosted CI or Actions.
- Use focused property and mutation testing for substantive rules. Follow the exact triggers in `docs/testing.md`; no blanket full-suite runs after unrelated edits.
- Do not create tests just to raise counts or coverage. Do not skip failing tests, weaken assertions, or add implementation-shaped tests to make a change pass.
- Report passed, failed, blocked, and not applicable separately. A scaffold check is not a product E2E pass.

## Coding workflow and communication

- Prefer Pi for coding. Route clear, bounded implementation tasks to an available Luna model after writing acceptance criteria. Use stronger reasoning for ambiguous architecture, security, concurrency, and unresolved failures. See `docs/development.md`.
- Give coding models only the required files and tools. No SSH identities, production credentials, unrestricted remote control, or unrelated home directories.
- Use [project-scoped Blaze](docs/operations/blaze-development.md) for development iteration; use pinned Cargo via `sh scripts/check` for final correctness checks. Review generated changes and run relevant checks before calling them done. Model size does not waive verification.
- Use RTK equivalents for shell reads, searches, Git, and test output. Use raw commands when output must be machine-readable, RTK cannot express the operation, or the command is output-free.
- Never print secrets, commit environment files, copy credential folders, deploy, or publish as part of a local scaffold task.
- Keep docs current: `docs/status.md` records actual implementation and outstanding gates; ADRs record decisions; do not duplicate their authority in chat exports.
