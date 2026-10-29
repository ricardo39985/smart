# Danny PC build operations

## Scope and trust boundary

Danny is a shared Windows PC with an existing Ubuntu-24.04 WSL2 build environment. The trusted local dispatcher remains the supported path for bounded Linux build jobs. It owns SSH authentication, host checks, resource admission, container lifecycle, and artifact verification. Coding models and jobs receive source and explicit commands, never SSH keys or the Docker socket. Danny is also the explicitly confirmed host for the separate operator-controlled native development API/database runtime; that runtime is not an arbitrary build backend. See [runtime operations](danny-development-api.md).

The local handoff is `/home/rick/Dropbox/PC/README.md`. Read its current instructions and the `Agent-Build-Environment/JOBS.md` contract before use; documents have historical sections, so verify live state. Do not copy that folder into the repository or archive: it contains credentials. Do not print keys, tokens, broad environment dumps, or auth files.

The dispatcher path on the current operator machine is:

```text
/home/rick/Dropbox/PC/Agent-Build-Environment/templates/generic-dispatch.py
```

The 2026-10-29 user request authorizes the workhorse feasibility check recorded below. It does not create blanket permission for host administration, deployment, production credentials, or unrelated jobs. Later work must fit its current user-authorized scope. The older Organizer standing-approval string is not SmartCindy authorization; never copy it into a new project manifest as if it were.

## Before submission

1. Review the source and exact command list. Construct a secret-free allowlisted archive, excluding `.git`, `.aws`, `.codex`, `.pi`, environment files, credentials, local caches, and unrelated projects. Reject links. Record the base commit plus working-tree snapshot identity and tar SHA-256; do not call an uncommitted snapshot an exact Git revision.
2. Select an already reviewed immutable Linux image ID compatible with the toolchain. Record the registry reference/digest separately. The dispatcher does not pull images. Provisioning an additional image is an operator action within authorized scope, not a command in a build job.
3. Record a unique job ID, source identity/hash, image ID, full commands, explicit artifacts, limits, and the truthful approval reference in the dispatcher's actual manifest schema. The pack's separate example request schema is not the deployed dispatcher schema.
4. Let the dispatcher check pinned host identity, free memory/disk, Docker readiness, unknown workloads, and aggregate reservations. Never bypass a safety-floor refusal, alter host pins, stop another job, or use a direct container backend instead.
5. If the existing WSL distro is stopped, `--start-wsl-approved` may start it only when covered by the current request. The 2026-10-29 test required this. WSL stopped automatically between probes; future agents must check rather than assume persistent uptime. No Docker restart, Windows reboot, new distro, or startup-task change follows from build authorization.

## Bounded workflow

Prepare a reviewed manifest and archive, then use the existing CLI. Replace the example paths and job ID with the exact reviewed attempt; do not reuse an old ID.

```sh
python3 /home/rick/Dropbox/PC/Agent-Build-Environment/templates/generic-dispatch.py submit \
  --manifest /absolute/path/to/manifest.json \
  --archive /absolute/path/to/source.tar \
  --approved --output-root /absolute/path/to/local-evidence
```

Add `--start-wsl-approved` only when authorized to start the existing distro. That flag may also be needed for status/wait/fetch after WSL automatically stops.

The dispatcher supports `status JOB_ID`, `wait JOB_ID`, `fetch JOB_ID`, and `cancel JOB_ID`. Use bounded waits and preserve each attempt. After a disconnect, inspect that job; do not start a duplicate. Cancellation must target only your job. Do not globally prune images, caches, containers, or volumes.

Jobs run as UID/GID 1000 in a read-only container with source mounted read-only, a bounded writable tmpfs copy, no network, no added capabilities, no new privileges, and CPU/memory/swap/PID limits. Commands run in `/work/source`; write returned artifacts under **`/work/out`**, not `/work/source/out`. Declare them in the manifest as `out/<filename>`. The pool is shared, not a full-host entitlement per job. Read current broker ceilings; do not infer availability from old measurements.

## Rust foundation recipe

The feasibility test selected the official `rust:1.96.0-slim-bookworm` Linux amd64 image. Registry metadata was resolved through the [Docker Hub tag API](https://hub.docker.com/v2/repositories/library/rust/tags/1.96.0-slim-bookworm).

- Linux amd64 digest and locally observed image ID: `sha256:bef0c0c8eee5b866fadb56bd83cdf4681921c3851f2b01dc13b621653a06a4f5`.
- Observed registry download size: 314,238,546 bytes; installed size: 1,234,299,001 bytes.
- It was added as an isolated build image; the existing Organizer image and services were preserved.

The networkless job explicitly selects the installed compiler (`RUSTUP_TOOLCHAIN=1.96.0`) and a writable job-local `CARGO_HOME`. This prevents the repository's optional formatting/lint components from triggering toolchain provisioning in a read-only, networkless worker. Run formatting/Clippy on an approved local development host unless a reviewed build image includes those components.

The current scaffold has no dependencies. Its remote recipe checks, builds a release library, compiles test binaries with `cargo test --no-run`, and builds rustdoc. The resulting `.rlib` and logs demonstrate a Rust scaffold build only. They do not prove product behavior, production Ubuntu compatibility, or application capacity.

The existing `/work` tmpfs is mounted `noexec`. A real attempt to execute Cargo's test binary returned permission denied. Do not bypass that restriction with an ELF loader, remount, alternate executable path, or broker edits. Running native tests, build scripts, and procedural-macro libraries will need a reviewed executable-workspace backend/image recipe before ordinary dependencies arrive. The initial dependency-free compile does not prove those workloads.

Once dependencies exist, prepare a reproducible image/cache or reviewed vendored dependency input before a networkless job. Do not enable unrestricted network or send personal Cargo credentials to make resolution succeed. Pin compiler/native ABI for release builds and verify outputs against the production target; the Debian probe image is development evidence, not a production release image.

The existing bounded build runner remains unchanged after the development runtime move. Its limitations below still apply; running the API and database natively on Danny does not establish that the runner can compile or execute tests there.

## Integration-test limitation

The current worker has no Docker socket. Therefore it cannot start Testcontainers, and Rust compilation does not establish database integration readiness. Run Testcontainers on a Docker-enabled approved development host. Native Rust test execution is also blocked by the workspace mount. A future Danny backend must explicitly support equivalent per-run isolation, resource accounting, lifecycle, and cleanup before it can host these tests. Never mount the host socket into an ordinary build container as a workaround.

There is no application frontend yet, so no real browser flow can be tested. Browser/MCP acceptance, live-provider integration, sustained-load performance, cold-boot startup, and unattended operation are separate future gates.

## Verification evidence

See [2026-10-29 foundation verification](../verification/2026-10-29-foundation.md). Keep failed attempts and successful results distinct. After success or failure, confirm no owned container, reservation, or queued work remains. Retain source/image hashes, terminal state, log and artifact hashes, and the exact check scope. Do not present an SSH connection alone as a workhorse pass.
