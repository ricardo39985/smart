# Development integration deployment — 2026-10-29

Rick requested activation using the [update runbook](../operations/native-integration-update.md),
including pending migrations and route verification while keeping the tunnel running.
This is Danny's Ubuntu-24.04 WSL2 development runtime, not Debian production.

## Artifact and verification gate

The immutable bundle is `.work/backend-integration-20261029`, based on commit
`4c61927f4c70d0961acb292529c92bf8b6089941` plus its recorded uncommitted snapshot.
All manifest build-input hashes matched current source before this run. Bundle
`SHA256SUMS` passed locally and independently after transfer to
`/opt/smartcindy/dev/staging/integration-20261029` on Danny.

Activated binary SHA-256:
`421e945352d5d319c1c7e3f393385dba10dd21b5ff61bae500d3a139b09f08be`.
This is the prepared unoptimized development ELF with debug sections stripped.
Danny reported x86-64 and GLIBC 2.39; all shared libraries resolved, satisfying
the artifact's highest required GLIBC 2.34.

Pinned Rust 1.96 verification on Inspiron:

```sh
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=1 rtk cargo test --locked --offline --test durable_chat_http --test memory_http --test context_http --test projects_http --test api_efficiency_http --test realtime_http --test push_http
```

Passed: **27 tests across seven suites**, RTK-reported duration 176.80 seconds,
using real disposable PostgreSQL 18/pgvector and local provider simulators. No
tests were skipped. No implementation was changed for this deployment; Blaze
development iteration was not needed. This run used the existing corrected test
fixtures recorded by the separate source continuation.

Initial sandbox Docker/SSH/service-bus/DNS checks were blocked. Expanded-access
operator commands succeeded. Pinned SSH host checks remained enabled. Protected
database configuration was loaded through transient systemd units with the
existing EnvironmentFile; credential contents were not printed or copied.

## Backup and activation

Read-only inspection found successful migrations 0001–0003 with matching SHA-384
checksums. The migration role had the necessary privileges; vector 0.8.7 was
installed and pg_trgm 1.6 was available. Therefore the pending upgrade was
**0004–0010**, rather than the runbook's estimated 0003–0010.

Only `smartcindy-danny-api.service` was stopped. Before migration, a private backup
directory `/opt/smartcindy/dev/backups/integration-20261029` was created containing:

- Custom-format `database.dump`, 69,125 bytes, SHA-256
  `252b7655c75641706575f9cb26bce780b8add9c7088f779c16ca097e0333f1f4`.
- `database.toc` and `SHA256SUMS`: `pg_restore --list` read the archive successfully,
  including migration history and table data; the dump checksum was rechecked.
- Previous `smartcindy` binary, SHA-256
  `83d1bd5f2e8d309e0b1b4aaf3a67c5a234693fab0e381c5b307135fdbcb45e2a`.

The staged new binary's explicit `migrate` command succeeded. Inspection then
verified successful versions 0001–0010 and matching checksums; pg_trgm was installed.
The prepared binary was installed under service owner/group smartcindy, mode 0755,
and atomically replaced on the runtime filesystem. Only the API was started.
The activated binary digest matched the prepared artifact exactly.

Archive inspection/checksum passed; a full restore drill and old-binary schema
compatibility were not exercised. Binary-only rollback remains uncertified. Use
the runbook's paired database/binary recovery process if required.

## Runtime verification

Passed readiness through Danny direct loopback, Inspiron's forwarded loopback,
and TLS-validated `https://dev.smartcindy.app/healthz`: each returned 200 and
`{"status":"ok"}`. Direct readiness passed again after synthetic cleanup.

Authenticated public smoke passed:

- Synthetic account/session creation and capability discovery.
- Project creation/listing/deletion and chat creation within the project.
- Case-insensitive title search, including a negative query.
- Empty selected-head generation lookup returning `{"generation":null}`.
- Memory creation, versioned correction, listing and deletion.
- Authenticated SSE resynchronization.
- Resource cleanup, empty listings, logout and revoked-session 401 rejection.

No generation was submitted and no paid provider call was made. After confirming
the account owned no messages or generations, exact-ID database cleanup removed
the synthetic account, sessions, resource tombstones and owned rows.

Capabilities reported text/image input true; image output/editing and all push
flags false. These are configuration declarations, not live-provider evidence.

Final running PIDs: API 5159; PostgreSQL 148; origin relay 1134896; Cloudflare tunnel
1113309. PostgreSQL, relay and tunnel PIDs matched preflight. No service configuration,
secret, port, connector or DNS changes were needed.

Passed: integrity/compatibility, seven contract suites, backup archive inspection,
migration verification/application, exact binary activation, three readiness paths,
authenticated route smoke, synthetic cleanup and tunnel continuity.
Failed final checks: none. Blocked deployment steps: none.
Not applicable to this activation: native Android/iOS acceptance, live paid
provider/embedding/push delivery, production Debian acceptance and full restore drill.
These remain separate gates; no such pass is claimed.

Local operator scripts remain under ignored `.work/integration-deploy-20261029`.
The immutable bundle manifest retains its original pre-activation metadata;
this record supplies the subsequent runtime evidence.
