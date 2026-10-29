# Local development API runtime

Current as of 2026-10-29. Rick selected this machine (Inspiron) for the development
server and tunnel; Danny remains the build workhorse. See
[ADR 0011](../decisions/0011-local-development-runtime.md).

## Runtime

| Resource | Local location |
| --- | --- |
| Native API | `/home/rick/.local/share/smartcindy-dev/bin/smartcindy` |
| API listener | `127.0.0.1:8787` |
| PostgreSQL listener | `127.0.0.1:55433` |
| Private runtime/data/userspace | `/home/rick/.local/share/smartcindy-dev` |
| Protected operating config | `/home/rick/.config/smartcindy-dev/dev.env` |
| Protected Apple key | `/home/rick/.config/smartcindy-dev/apple-signin.p8` |
| Migration snapshot/roles | `/home/rick/.local/share/smartcindy-dev/backups/local-move-20261029` |
| PostgreSQL user service | `smartcindy-local-postgres.service` |
| API user service | `smartcindy-local-api.service` |
| Existing connector | `smartcindy-dev-tunnel.service` |

Private directories are mode 0700; configuration, keys and database/role backups
are mode 0600. The selected Dropbox configuration store remains separate from
these operating copies; build jobs must not read either. Never print environment
files, role dumps, database contents, session tokens or key material.

PostgreSQL runs natively using the previously pinned PostgreSQL/pgvector userspace
from `.work/runtime/postgres-rootfs.tar.gz`, extracted into the private runtime.
The `bin/pg-native` launcher invokes Bubblewrap with a read-only userspace, host
passwd/group identity lookup, `/proc` and `/dev`, a writable private data mount at
`/var/lib/postgresql/data` and private socket mount at `/tmp`. Network is shared so
PostgreSQL can bind its literal loopback address. No Docker container runs this
database. Local socket administration uses `smartcindy_local_admin` within the
private mode-0700 socket directory; TCP authentication uses SCRAM and preserved
database roles. Do not expose this socket directory or bind PostgreSQL publicly.

Reviewed unit copies are [API](../../ops/smartcindy-local-api.service) and
[PostgreSQL](../../ops/smartcindy-local-postgres.service). They are installed under
`/home/rick/.config/systemd/user`; both are enabled. Resource ceilings remain API
2 CPUs/1 GiB and PostgreSQL 1 CPU/512 MiB, with no service swap. These are limits,
not measured capacity guarantees.

## Operator checks

```sh
systemctl --user show smartcindy-local-api.service smartcindy-local-postgres.service smartcindy-dev-tunnel.service -p Id -p ActiveState -p SubState -p NRestarts
journalctl --user -u smartcindy-local-api.service -u smartcindy-local-postgres.service --since '30 minutes ago' --no-pager
curl --fail --silent --show-error http://127.0.0.1:8787/healthz
curl --fail --silent --show-error -A SmartCindy-Development-Probe/0.1 https://dev.smartcindy.app/healthz
```

Use bounded sanitized log inspection; SQL errors may include private values.
Start PostgreSQL before API; stop API before PostgreSQL. Only restart the failed
component. Keep the connector running during routine server work:

```sh
systemctl --user start smartcindy-local-postgres.service
systemctl --user start smartcindy-local-api.service
systemctl --user stop smartcindy-local-api.service
systemctl --user stop smartcindy-local-postgres.service
```

The API does not migrate on startup. For a reviewed schema update, stop API writes,
make a private database backup, preserve the binary, and run the staged binary's
explicit `migrate` through the protected systemd EnvironmentFile loader. Never put
the database URL in arguments/logs. Verify the full chain before atomically
activating a new binary. User units follow the login lifecycle; no linger or
unattended uptime guarantee was introduced.

## Routing and recovery

Cloudflare reaches the local API directly at the existing loopback port. The tunnel
retains its original configuration, credential, DNS record and running process.
The old `smartcindy-danny-origin.service` is stopped/disabled (its linked installed
unit was removed by disable); the repository unit remains historical evidence.
Danny's `smartcindy-danny-api.service` and `smartcindy-danny-postgres.service` are
stopped and disabled. Remote data/binaries/backups and workhorse configuration
remain intact. Do not restart those old services as a second writable authority.

The frozen pre-move backup is preserved both locally and on Danny. A full logical
restore was exercised for this move. After cutover, local writes are authoritative:
never switch to the frozen remote database without preserving/reconciling local
writes. For recovery, stop API writes, make a fresh private local backup, and use
a reviewed compatible binary/database pair. This migration snapshot does not
establish scheduled encrypted off-machine backups or production recovery readiness.
