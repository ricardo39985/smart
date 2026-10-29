# 0004: Native Debian deployment; Docker permitted for development

Date: 2026-10-29

Status: Accepted by the user's explicit choices.

## Context

The user selected Debian on Lightsail after withdrawing the FreeBSD proposal, then clarified: no Docker on deployment; Docker may be used in development when useful. This supersedes the Ubuntu deployment direction and the proposed production Compose packaging.

## Decision

- Production is one Debian Lightsail instance. Run the Rust API/bounded workers, PostgreSQL/pgvector, PgBouncer, Caddy and future private embedding process as native services, using systemd where applicable. Retain encrypted off-instance backups.
- Do not require or install Docker or Compose for production. Deliver versioned native Rust artifacts and reviewed native service/database configuration.
- Docker remains permitted for development builds and isolated tests. Retain the real PostgreSQL 18/pgvector Testcontainers suites and their pinned image. Development container images are not deployed artifacts.
- Danny remains the intended build/test workhorse through a reviewed execution environment. Preserve resource accounting, credential boundaries and unrelated workloads. Its current runner restrictions are unresolved; this decision does not enable a new backend or authorize bypasses.
- Pin the supported Debian Lightsail blueprint/release and CPU architecture in the provisioning record before release. Build against a compatible Debian userspace and test on the actual deployment release. Ubuntu development or container test success does not establish native Debian release compatibility.
- Preserve the other Rust/PostgreSQL/provider/memory selections. Verify OpenVINO packaging and execution on the chosen Debian release when implementing embeddings. No alternate embedding runtime or FreeBSD port is selected.
- No GitHub Actions or replacement hosted CI.

## Consequences

Containerized development and native production are intentional separate environments. Database versions, extensions, migrations and behavior must match. Release checks must cover native startup, migration privileges, service restart/shutdown, private storage, backup/restore and rollback. A Testcontainers pass remains development evidence.

AWS currently lists Debian 13 and 12 blueprints; exact release and regional availability remain provisioning checks. [Lightsail blueprint documentation](https://docs.aws.amazon.com/lightsail/latest/userguide/compare-options-choose-lightsail-instance-image.html).

## Verification

Only documentation and contributor instructions changed. No application code, test harness, host packages, remote runner, cloud instance or tunnel was changed. Historical test evidence remains unchanged. Native Debian execution and Lightsail deployment are pending.
