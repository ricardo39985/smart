# Development integration update

Subsequent placement: the verified application/database were moved from Danny to
Inspiron. Danny's frozen runtime is stopped/disabled. Use
[local operations](local-development-api.md) and
[move evidence](../verification/2026-10-29-local-runtime-move.md) for the current
runtime; the Danny activation below is historical evidence, not an instruction
to restart a second writable server.

Activated on Danny on 2026-10-29: migrations 0004–0010, the prepared binary,
direct/forwarded/public readiness and authenticated public smoke passed.
PostgreSQL, origin relay and tunnel stayed running. See the
[activation evidence](../verification/2026-10-29-native-integration-deployment.md).
Preparation and access-limit notes below describe the earlier handoff state.

Prepared 2026-10-29 following Rick's instruction to finish the backend while the
frontend handoff goes to another chat. The new development binary is built, but
had **not** yet been transferred, migrated, or activated on Danny at handoff.

## Prepared artifact

Directory: `/home/rick/Documents/Code/smartcindy/.work/backend-integration-20261029`.
It contains `smartcindy`, `source.tar.gz`, `api.md`, `openapi.json`, `manifest.json`
and `SHA256SUMS`. The manifest records each build input's digest and the base
commit `4c61927f4c70d0961acb292529c92bf8b6089941`. This is an uncommitted source
snapshot, not a release revision.

Binary SHA-256:
`421e945352d5d319c1c7e3f393385dba10dd21b5ff61bae500d3a139b09f08be`.

Built with `cargo build --locked --offline --bin smartcindy` on pinned Rust 1.96.
The packaged copy has debug sections removed; it remains an unoptimized
development build. It is Linux x86-64 ELF requiring GLIBC through 2.34. Danny was
previously verified on GLIBC 2.39; confirm the current target and shared libraries
before activation. This inspection is not execution on Danny or Debian production
acceptance. Source archive includes all Rust sources, Cargo/toolchain inputs and
SQL migrations, with no credentials, Git metadata or environment files.

## Current access limitations

Continuation update: the approved local Docker host was accessible with normal
operator permissions. All seven affected suites in step 2 below have now passed
against real PostgreSQL/pgvector (27 scenarios). The earlier access failures below
remain historical evidence. Only test fixtures changed; the prepared binary and
embedded migrations remain the same. Rick is coordinating activation; no remote
transfer, migration or service operation was performed by this continuation.

- Local Docker preflight failed with permission denied at `/var/run/docker.sock`.
  Real PostgreSQL/Testcontainers tests were not launched.
- Public health probe failed DNS resolution in this session. This is not evidence
  that the server or tunnel is down.
- [Danny runtime operations](danny-development-api.md) reserves remote service and
  credential access for the trusted operator. The available bounded build
  dispatcher is not a deployment interface. No direct SSH credential access or
  host-control workaround was used.

## Activation sequence for the trusted operator

Use the existing operator access and [runtime guide](danny-development-api.md).
Do not change secrets, services, ports, the origin relay or the Cloudflare connector
as part of this update.

1. Verify the bundle with `sha256sum -c SHA256SUMS` locally and independently after
   transfer. Stage the binary outside the active path on Danny's runtime filesystem
   with executable permissions for the service account. Verify x86-64/GLIBC/library
   compatibility and PostgreSQL readiness before any service stop.
2. On an approved Docker-enabled development host, run the affected real PostgreSQL
   contracts: `durable_chat_http`, `memory_http`, `context_http`, `projects_http`,
   `api_efficiency_http`, `realtime_http`, and `push_http`. The last deployed profile
   build predates all these additions. Resolve actual failures before activation;
   do not call the existing compilation evidence behavioral acceptance.
3. Inspect `_sqlx_migrations` using the protected direct database connection. Verify
   existing migration checksums and extension availability. Expected upgrade from
   the last recorded runtime is 0003 through 0010, not just 0010. `vector` is already
   required; `pg_trgm` must be available in the native PostgreSQL userspace and
   installable by the migration role. Do not edit applied migrations.
4. Make a recoverable database backup and preserve the currently running binary
   before migration. Stop only `smartcindy-danny-api.service`. PostgreSQL and the
   tunnel stay running. Migration 0010 creates nonconcurrent indexes; apply in this
   development maintenance window rather than promising lock-free installation.
5. Invoke the **new staged binary** with `migrate`, supplying the existing protected
   `MIGRATION_DATABASE_URL` through the operator's environment loader. The binary
   embeds the entire ordered SQLx migration chain. Do not expose the URL in shell
   arguments/logs, print/source credentials into an agent context, or assume normal
   API startup migrates. A root-run transient systemd unit with the existing
   `EnvironmentFile` can pass configuration without printing it; respect the actual
   migration role's privileges. Stop on nonzero status and inspect migration state.
6. Only after migration succeeds, stage with the `smartcindy` service owner/mode
   0755 and atomically replace `/opt/smartcindy/dev/bin/smartcindy`, following the
   existing runtime guide. Start only `smartcindy-danny-api.service`.
7. Verify direct Danny readiness, forwarded Inspiron readiness and public HTTPS
   readiness. Run a synthetic authenticated no-provider smoke: capabilities,
   project/chat create/list/title search, empty head-generation lookup, memory
   create/edit/delete, event feed, cleanup/logout/revoked-session rejection. Native
   frontend/provider/push delivery acceptance is separate.

Provider and embedding credentials are not part of this bundle. Existing runtime
configuration is preserved; capabilities remain false for unavailable providers.
An updated binary alone does not enable live text/image or native push providers.

## Failure and rollback

Keep API stopped while migration state is uncertain. SQLx transactions protect
individual migrations, but earlier successful migrations may remain applied if a
later one fails. Do not drop new tables or erase migration history to retry.
Inspect and resolve that failure through the operator workflow.

Schema compatibility of the older binary after these migrations has not been
executed. A binary-only rollback is not certified. If repair is unsafe, restore the
verified pre-update database backup and preserved binary together while API writes
remain stopped, then verify readiness. Do not revert the database after allowing
new writes without first preserving/reconciling them.

## Evidence

Passed: pinned locked offline binary build (12.92 seconds), ELF dependency/version
inspection, bundle hashes. Prior all-target checking and Flutter static analysis
are recorded in the [source handoff](../verification/2026-10-29-native-integration-source.md).
Blocked: Docker preflight and public DNS probe. Not run: remote transfer, migration,
activation, runtime smoke, native/provider acceptance. No service was changed.
