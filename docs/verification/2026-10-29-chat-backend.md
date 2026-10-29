# Durable chat backend verification — 2026-10-29

This is a local backend working-tree snapshot, with no commit, push, persistent database migration or deployment. [Snapshot hashes and structured evidence](evidence/2026-10-29-chat-backend.json) identify the reviewed files. Snapshot SHA-256: `87782977e920068ccf42a3571cd530b637ae399b365f603284d3a425819c79e9`.

## Passed

- `sh scripts/check`: Rust 1.96 formatting, Clippy with denied warnings, all 10 public HTTP scenarios (9 chat/resource journeys plus 1 restart journey), doctests and warning-free documentation. The final full test run took 28.43 seconds. A subsequently added unknown-field assertion passed in its affected complete HTTP scenario; formatting and strict all-target lint passed afterward. No additional production logic changed after the full run.
- Production SQLx migrations on disposable PostgreSQL 18/pgvector Testcontainers, Linux amd64 manifest pinned to `sha256:ad249f9fb9572979643a47694e59b9a8cdcdf6abc6accebf49bb80ad5f20ff9b`. Each scenario has its own database/container. Docker errors are failures, never skipped.
- 24 fixed-seed public cursor/limit property cases, seed 73641981. Concurrent duplicate acceptance, actual HTTP streaming including fragmented/CRLF/multiline frames, restart history/replay, no repeated paid submit after an expired running lease, ownership, session revocation and invalid credentials, edits/regeneration, selected-branch context, cancellation/deletion, private images and base64 image editing are exercised. Only external paid HTTP protocols are simulated.
- Three behavioral red/green regressions: branch continuation removed a previous snapshot head (0 items versus expected1); deleted-chat images remained undeletable (409 versus expected404); stream subscriptions were unbounded (200 versus expected429). The same assertions pass after fixes. The stream case also checks capacity returns after disconnect. The unknown-field assertion passed initially and is not counted as a red reproduction.
- `cargo mutants --in-place --baseline run --timeout 90 --build-timeout 120 --re '<selected expiry/owner/chat/page guard mutations>' -- --test durable_chat_http`: cargo-mutants27.1.0 tested four selected mutants, all caught, no survivors/timeouts/unviable results. The selected guard expressions stayed unchanged afterward. This does not establish mutation coverage for embedded SQL, all leases, or all security rules.
- OpenAPI3.1 formal validator and local reference/path checks; the machine contract covers all14 router paths. This is schema/route documentation validation, not generated Flutter runtime acceptance.
- Blaze project-local installation checks: archive hash/version, script shell syntax, idempotent existing-install path and absolute invocation from /tmp. Stable `sh scripts/dev check --locked` passed. See [Blaze compatibility](../operations/blaze-development.md) for exact verified failures and toolchain separation.

## Failed / blocked

Blaze stable build failed on an unsupported standard-library writable static. The separately checksum-verified candidate0.20261029.4 failed on an Axum async coroutine layout. SQLx native macro expansion was configured using Blaze's documented per-package setting; sandboxing was not disabled. Cargo builds/checks pass. No Blaze build/test or speedup claim is made.

## Not run and remaining limits

No live paid API calls/charges, native Flutter journey or lazy-scroll implementation, iOS device build, persistent development origin startup, production migration/deployment, benchmark/capacity certification, backup/restore drill, or comprehensive security stress audit. Provider contracts were checked against official documentation; simulator passes do not establish live compatibility. Native lists must consume bounded branch pages and preserve their scroll anchor when older history loads. No GitHub Actions or hosted CI was added. Danny's mount/socket restrictions were preserved.

Temporary execution logs are `/tmp/smartcindy-final-check.log`, `/tmp/smartcindy-final-lint.log`, `/tmp/smartcindy-http-green.log`, `/tmp/smartcindy-recovery.log`, `/tmp/smartcindy-branch-red.log`, `/tmp/smartcindy-storage-red.log`, `/tmp/smartcindy-stream-bound-red.log`, `/tmp/smartcindy-mutants.log` and `/tmp/smartcindy-blaze-*.log`. Durable facts and hashes are retained here; temporary logs can disappear.
