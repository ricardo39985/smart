# 0011: Local development runtime; Danny remains a workhorse

Date: 2026-10-29

Status: Accepted by Rick's explicit instruction and activated.

## Context

The development API/database on Danny were healthy, but the LAN SSH relay repeatedly
timed out and interrupted public access. Rick requested that Danny remain a build
workhorse while the server and tunnel run on this Inspiron machine.

## Decision

Run the development API and PostgreSQL 18/pgvector as local user systemd services.
Use the existing verified native API binary and pinned PostgreSQL userspace, with
Bubblewrap providing a read-only filesystem view and private writable data/socket
mounts. No Docker daemon/container is required for this runtime and no system
packages or administrator privileges were needed. The user service owns the private
cluster. Keep protected configuration outside the repositories.

Keep the existing local Cloudflare connector and its loopback origin port 8787.
Disable the obsolete SSH origin relay and Danny's API/database services; preserve
their data/backups. Keep Danny's bounded build environment unchanged. Production
Debian/Lightsail architecture and the build/test boundaries in ADR 0004 remain.

## Consequences

Development availability depends on this machine and the user's login session,
without a LAN/Danny dependency. Sleep, logout and network changes can still interrupt
availability. No linger, firewall, DNS, credential or unrelated service changes
follow from this decision. Rollback after local writes requires preserving and
reconciling those writes; the frozen Danny database is not a current replica.

## Verification

The final frozen database and roles were restored locally, all exported table
counts matched, migrations verified, direct/public readiness passed, and both local
and public authenticated no-provider route smoke passed. The tunnel kept its
original PID without restart. See [operations](../operations/local-development-api.md)
and [migration evidence](../verification/2026-10-29-local-runtime-move.md).
