# Danny development API runtime

Last verified: 2026-10-29. This is an operator-controlled development service on Danny's existing Ubuntu-24.04 WSL2. It is not production and does not enable arbitrary native build jobs. The bounded build runner remains unchanged; its `noexec` workspace and lack of a Docker socket still prevent native test execution and Testcontainers.

## Runtime and route

The API and PostgreSQL run as native systemd services inside WSL2 on `Dannys-Old-But-Gold-Pc`, under the dedicated `smartcindy` account (UID 999), from the separately owned `/opt/smartcindy/dev` runtime. Units are installed on Danny as `smartcindy-danny-postgres.service` and `smartcindy-danny-api.service`. PostgreSQL listens only on `127.0.0.1:55433`; API listens on `127.0.0.1:8787`. PostgreSQL starts before the API. RootDirectory uses the pinned PostgreSQL 18/pgvector userspace; no PostgreSQL Docker container runs on Danny.

The existing path remains Cloudflare → the `rick-Inspiron-16-5625` user cloudflared service → its SSH local forward on `127.0.0.1:8787` → Danny Windows loopback → WSL API. The relay is `smartcindy-danny-origin.service`; its pinned SSH host keys and identity stay on Inspiron. `wsl.exe ... sleep infinity` keeps the distro alive while connected. This route requires both machines, LAN, their user services, and awake Windows; it is not cold-boot or unattended availability. No Cloudflare DNS/credential changes were made.

Protected configuration is root-owned `/opt/smartcindy/dev/secrets/dev.env` (mode 600); it contains development database URLs and listen address only. Only the runtime and trusted operator may read it; never print it, commit it, or pass it to coding models. Do not expose the database or bind either service publicly.

## Operator commands

Run Danny commands only through the existing trusted operator-controlled WSL access, after verifying the intended host/distro. Do not give coding agents SSH access or credentials. On Danny:

```sh
sudo systemctl status smartcindy-danny-postgres.service smartcindy-danny-api.service
sudo systemctl start smartcindy-danny-postgres.service
sudo systemctl start smartcindy-danny-api.service
sudo systemctl stop smartcindy-danny-api.service
sudo systemctl stop smartcindy-danny-postgres.service
curl --fail --silent --show-error http://127.0.0.1:8787/healthz
```

Start PostgreSQL before migrations or API startup. Stop API before PostgreSQL. For routine restart, restart PostgreSQL first only when needed, then restart API:

```sh
sudo systemctl restart smartcindy-danny-postgres.service
sudo systemctl restart smartcindy-danny-api.service
sudo systemctl status smartcindy-danny-postgres.service smartcindy-danny-api.service
```

On Inspiron, inspect/restart only the two owned user services:

```sh
systemctl --user show smartcindy-danny-origin.service smartcindy-dev-tunnel.service
systemctl --user restart smartcindy-danny-origin.service smartcindy-dev-tunnel.service
```

## Reviewed binary update

Build and review a compatible ELF artifact on an approved host. The current runtime binary was built on Inspiron; its verified SHA-256 is `aa6072f9ecf53ddf41d0c38aa8f194d285bb58257a0d78f1c5fa1f6075371c63`. Danny runtime compatibility, migrations, and HTTP behavior were verified, but compilation and full Rust tests on Danny were not. Never describe this as a Danny build/test pass.

For an update, verify the artifact's provenance, compatibility, and independently computed SHA-256 before transfer using the trusted operator path. On Danny, stop only the API, stage the reviewed artifact in the same filesystem, set ownership/mode appropriately, atomically replace `/opt/smartcindy/dev/bin/smartcindy`, then start the API and verify both direct and public health. Preserve the old version for operator-controlled rollback; do not mutate service files or secrets as part of a binary update. Example destination-side sequence (with the staged path and expected digest supplied by the operator):

```sh
sudo systemctl stop smartcindy-danny-api.service
sudo sha256sum /path/to/staged/smartcindy
sudo install -o smartcindy -g smartcindy -m 0755 /path/to/staged/smartcindy /opt/smartcindy/dev/bin/smartcindy.next
sudo mv /opt/smartcindy/dev/bin/smartcindy.next /opt/smartcindy/dev/bin/smartcindy
sudo systemctl start smartcindy-danny-api.service
curl --fail --silent --show-error http://127.0.0.1:8787/healthz
curl --fail --silent --show-error https://dev.smartcindy.app/healthz
```

Do not proceed unless the staged digest matches the reviewed expected digest. For schema changes, review migration compatibility and run the intended migration explicitly; the API never migrates implicitly. Resolve a supported build/test recipe separately before using Danny for execution. Do not silently revert runtime execution to Inspiron.

## Verified behavior and limits

On 2026-10-29 direct Danny and public HTTPS health returned 200 `{"status":"ok"}`; `/v1/chats` returned generic 401 and `/` generic 404. TLS validation was enabled and public responses included Cloudflare Ray IDs. Health performs bounded database readiness; it does not cover providers or workers. Focused health HTTP tests included real PostgreSQL and a closed-pool 503. Formatting, strict all-target Clippy, locked binary build, OpenAPI validation (15 paths), and the sibling Flutter pinned checks passed on Inspiron. No live provider requests/charges or native client journeys were performed. See [sanitized evidence](../verification/evidence/2026-10-29-development-api.json) and [backend verification](../verification/2026-10-29-development-api.md).

An authenticated smoke check executed on Danny through the public hostname also passed session creation, capability discovery, chat creation/pagination/rename/read, empty-message listing, deletion, logout and revoked-session rejection. Its synthetic account/chat rows were removed afterward; no provider requests were made. See [remote public smoke evidence](../verification/evidence/2026-10-29-danny-public-smoke.json).

Development resource caps are limits, not capacity guarantees. Shared broker preflight showed no running containers, queued/active jobs, or reservations; no broker settings were changed. No production instance, other services, Docker resources (except the retained rollback volume), Windows tasks, reboot, firewall, global packages, or unrelated tunnels were changed. Native Debian production acceptance remains separate; extracting a development userspace does not establish it.
