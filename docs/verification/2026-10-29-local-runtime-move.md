# Local runtime move — 2026-10-29

Rick requested keeping Danny as a workhorse and moving the server onto this machine
alongside the tunnel after a read-only log investigation found recurring relay
failures. See [ADR 0011](../decisions/0011-local-development-runtime.md) and
[current operations](../operations/local-development-api.md).

## Findings before the move

The three-hour journal window ending around 19:00 Eastern showed six SSH relay
server timeouts and one SSH connection timeout. The latest server timeout was
18:58:27, followed by origin connection refusals in Cloudflare logs. A public
readiness probe returned 502 and the forwarded loopback port refused connection.
Direct SSH initially timed out, then recovered. Public/forwarded readiness also
recovered without an operator restart.

Danny's API PID 5159 and PostgreSQL PID 148 remained running with zero automatic
restarts. Their recent journals had no error/warning matches; PostgreSQL entries
were normal checkpoints. No kernel OOM events were found. The WSL kernel journal
contained 105 Weston RDP-backend segfaults between 16:00:57 and 18:58:24 Eastern.
Those GUI crashes were not shown to cause the network failures. The API journal
contains lifecycle events rather than detailed request diagnostics, so clean
journals do not certify every provider request.

## Transfer and restore

Prepared a private local native runtime and rootless PostgreSQL userspace using
Bubblewrap. No system packages, sudo configuration or unrelated services changed.
PostgreSQL userspace archive SHA-256:
`3642faaddaf6a6708ca1a0df0e7a32a95cdb54e30ea7b75bec6b2806bdf18790`.
Safe extraction normalized absolute symlinks to internal relative targets and
excluded device nodes. This is the same native userspace family used on Danny.

Only Danny's API was stopped to freeze writes. Exported current database and roles
into private `/opt/smartcindy/dev/backups/local-move-20261029`, verified the custom
archive catalog, and streamed files directly to private local backup/configuration
paths without printing their contents. Database archive SHA-256:
`d26e82430acbb21b563f9a304ac728388b7768d750f5c097fd29e1a84584cf3b`.
All transferred backup file hashes matched. Existing runtime configuration and
Apple signing key were preserved; only the operating key path and temporary/final
listen address were adjusted locally. No provider credentials were changed.

Restored roles and the complete logical database into local PostgreSQL 18.6/pgvector.
Verified matching source/destination counts before testing and again after cleanup:

| Records | Count |
| --- | ---: |
| Users | 1 |
| Sessions | 1 |
| Account identities | 1 |
| Chats | 1 |
| Messages | 10 |
| Generations | 5 |
| Attachments | 1 |
| Projects | 1 |
| Memories | 0 |

All migrations 0001–0010 were successful. The exact API binary's explicit migrate
command succeeded locally, checking SQLx's existing migration checksums. Binary
SHA-256 remained
`421e945352d5d319c1c7e3f393385dba10dd21b5ff61bae500d3a139b09f08be`.

## Cutover and outcome

Local API readiness and authenticated no-provider smoke passed on temporary port
18787 before cutover. Danny's API/database services were then stopped/disabled,
the obsolete local SSH relay was stopped/disabled, and local API switched to
127.0.0.1:8787. Enabled both local runtime user services.

Public HTTPS and direct local readiness passed. Repeated the authenticated smoke
through `https://dev.smartcindy.app`: signup, capability discovery, project/chat
creation/listing/deletion, positive/negative title search, empty head-generation
lookup, memory creation/correction/listing/deletion, SSE resync, logout and revoked
session rejection all passed. No provider generation was submitted. Both synthetic
accounts and owned rows were removed; original table counts still matched.

Final local state: API PID 1199609, PostgreSQL PID 1199132, both active/enabled with
zero automatic restarts. Their new journals had no error/warning matches. Tunnel
PID 1113309 remained active/enabled with zero restarts throughout. No connector,
DNS, firewall, host power, unrelated tunnel or workhorse changes were made.

Passed: backup integrity, full logical restore, record counts, migration checks,
temporary/final direct readiness, public readiness, two authenticated route smokes,
cleanup and tunnel continuity. Failed final checks: none. Blocked steps: none.
Not applicable: new implementation tests, live paid-provider/device/push acceptance,
Debian production acceptance, unattended availability and scheduled backup recovery.
The 27 affected PostgreSQL contract tests had already passed before the preceding
binary deployment; the same application binary was moved without code changes.

Private operator helpers remain under ignored `.work/local-runtime-move-20261029`;
data/credentials/backups reside outside the repository. Local data is authoritative
after cutover; the preserved Danny snapshot is a rollback artifact, not a replica.
