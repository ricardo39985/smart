# Development API runtime verification

Date: 2026-10-29. Sanitized machine-readable responses and runtime identity: [evidence JSON](evidence/2026-10-29-development-api.json).

## Result

Explicit user correction reaffirmed Danny's PC as the execution host. The API and database now run as native systemd services in its existing Ubuntu-24.04 WSL2, from the owned `/opt/smartcindy/dev` runtime under dedicated UID 999. The API and PostgreSQL units are enabled and active. This is development only; no production instance was involved.

The runtime binary was built on Inspiron and has SHA-256 `aa6072f9ecf53ddf41d0c38aa8f194d285bb58257a0d78f1c5fa1f6075371c63`. Native compatibility, migration execution, direct HTTP health, and public route behavior were verified on Danny. Compilation and full Rust tests were not run on Danny. The bounded build runner was not modified and still has `noexec`/no-Docker-socket constraints.

PostgreSQL 18/pgvector userspace came from `pgvector/pgvector:pg18@sha256:ad249f9fb9572979643a47694e59b9a8cdcdf6abc6accebf49bb80ad5f20ff9b`; the exported gzip-1 source archive SHA-256 was `3642faaddaf6a6708ca1a0df0e7a32a95cdb54e30ea7b75bec6b2806bdf18790`. Extraction skipped device/FIFO nodes and converted absolute symlinks to equivalent relative in-root links before filtered extraction. systemd `RootDirectory` supplies the userspace; no Docker container runs there. The data directory is inside that runtime root. PostgreSQL initialization used SCRAM host authentication and local trust, with its password supplied through a temporary protected file removed afterward. A read-only startup issue was fixed with the required `ReadWritePaths=+/var/lib/postgresql`; actual startup was observed afterward.

The public route uses the existing Inspiron Cloudflare connector and the strict-host-pinned SSH origin relay to Danny Windows loopback and WSL. The SSH identity remains only on Inspiron; no agent forwarding. WSL is kept alive only while the relay is connected. Both computers, LAN and user services must be available; no cold-boot/unattended guarantee is established. No Cloudflare credentials or DNS were changed.

## Checks

- Direct Danny `/healthz`: 200 `{"status":"ok"}`; same binary digest verified.
- Public HTTPS `/healthz`: 200, valid TLS, Cloudflare Ray ID; `/v1/chats`: generic 401; `/`: generic 404. Loopback relay matched these application responses.
- Health is unauthenticated and performs a real `SELECT 1` with one-second acquisition/query bounds; responses are no-store and report 200/503. It does not report provider or worker health.
- Focused health HTTP tests passed with real PostgreSQL, closed-pool 503, and chat authentication 401.
- Formatting, all-target strict Clippy, locked binary build, and OpenAPI 3.1 validation (15 paths) passed on Inspiron. Flutter `sh scripts/check` passed pinned-toolchain format/analyze. Historical 10 backend scenarios remain evidence for their earlier snapshot, not a claim they were rerun here.
- Shared broker preflight showed no running containers, queued/active jobs, or reservations and unchanged pool ceilings.
- An authenticated smoke scenario ran on Danny against the public Cloudflare hostname: session creation, capability discovery, chat creation/pagination/rename/read, empty-message listing, chat deletion, logout and revoked-session rejection all passed. The generated synthetic account, sessions and empty chat were removed afterward. [Sanitized remote evidence](evidence/2026-10-29-danny-public-smoke.json) contains only status codes, paths and Cloudflare Ray IDs; no passwords or tokens.

No provider keys were configured, live AI calls/charges or native journeys were made, and no production deployment occurred. The development caps are not capacity guarantees. No broker edits, global packages, unrelated Docker resources, Windows tasks, reboot, firewall changes, or other services were changed. The stopped initial local API and database were not left active; the owned Docker development database was stopped with restart disabled and its volume retained only for rollback. The old local API unit is superseded, not an active runtime path.

Danny remains the runtime execution host. Resolve a supported build/test recipe explicitly before future execution work; do not silently switch back to Inspiron. The development image extraction does not establish native Debian production acceptance. See [Danny runtime operations](../operations/danny-development-api.md).
