# Foundation verification — 2026-10-29

Status: local scaffold checks and Danny compile/artifact verification passed. Native test execution and Testcontainers remain unsupported by the current remote backend. This file records engineering evidence, not product acceptance.

This is historical evidence of the first foundation snapshot. The subsequent [Flutter and local-checks decision](../decisions/0002-flutter-client-and-local-checks.md) removed the unrun Actions workflow and replaced the planned in-repository browser frontend with a separate native app. Preserved snapshot JSON remains unchanged for provenance; it does not override current architecture or contributor instructions.

## Pi coding

Pi 1.4.0 reported `openai-codex/gpt-6-luna` available. Credential-free readiness check returned `ready` using existing OAuth. Luna implemented the specified scaffold and CI with low reasoning and read/edit/write tools, without MCP/extensions, shell execution, or persistent sessions. The primary agent reviewed the generated files and performed verification independently.

## Local foundation

Rust 1.96.0: formatting, Clippy with warnings denied, compilation through the test harness, doc-test harness, and rustdoc with warnings denied passed. Shell syntax, local documentation links, and source/config whitespace checks passed. No runtime or dev dependencies, product functions, or runnable service exist. There are zero product tests; empty harnesses are not behavioral acceptance.

The initial, unrun workflow pinned the GitHub checkout action to `11d5960a326750d5838078e36cf38b85af677262`, resolved from the official repository's `refs/tags/v4` on this date. That workflow was subsequently removed at the user's request before any push or execution. No Actions workflow remains; current checks are local.

## Danny attempts

Pinned SSH identity checks succeeded. The existing Ubuntu-24.04 WSL distro was stopped; the guarded dispatcher started it for this request. Live preflight observed Docker 29.9.2, cgroup v2, 20 Docker CPUs, about 31.3 GiB Docker memory, and no active/queued pool jobs. These are dated observations, not future availability guarantees.

### Attempt 1: existing image probe — failed

Job: `smartcindy-probe-20261029-01`.

The container was created with no network, read-only source, UID 1000, 2 CPUs, 1 GiB memory, 64 PIDs, and a 120-second deadline. Its synthetic native compile probe failed because no C compiler was found on the default job path. Do not count it as a build pass.

- Source archive SHA-256: `4b89adba98735b39141abdd874e04b79ec72cabedfbaed78bc292ee2c61a26e5`.
- Existing image: `sha256:04597109ff53aec023362198276065f1a8f8e4af0d9124413f58d09d02e1f744`.
- Failed log SHA-256: `4fca9f9187ec579b6e2411e48cd9310242e6e1b2df96d7ab09ef6febe56032cd`.
- Raw local evidence: `/tmp/smartcindy-danny-probe/evidence/smartcindy-probe-20261029-01/` (temporary, not durable documentation).

### Attempt 2: dedicated Rust image, test execution — failed

Job: `smartcindy-rust-20261029-01`.

The official Rust 1.96.0 slim Debian image was resolved by immutable digest and provisioned without changing existing images/services. Source is an allowlisted uncommitted snapshot of Cargo files, library source, and selected documentation. The base Git commit identifies ancestry; the archive hash identifies actual submitted content.

- Source archive SHA-256: `db16b706c49ae600507b66ed055ece106f5cd8d8818ed0c9322f2b4afcb1c5b8`.
- Image: `sha256:bef0c0c8eee5b866fadb56bd83cdf4681921c3851f2b01dc13b621653a06a4f5`.
- Limits: 2 CPUs, 1 GiB RAM/no additional swap, 64 PIDs, 256 MiB workspace tmpfs, 180 seconds, 10 MiB artifacts.
- Raw evidence destination: `/tmp/smartcindy-danny-rust/evidence/smartcindy-rust-20261029-01/`.

Compilation and release-library build passed, but Cargo could not execute the generated test binary: permission denied. The workspace tmpfs is `noexec`; no mount, ELF-loader, or broker workaround was used. This attempt is a failed test-execution probe, not a test pass.

- Log SHA-256: `c7fe3f070c533c12d8c96cbbbedcdc01fc6dc249d1d4a784527a6ba7554a59dc`.

### Attempt 3: compile-only, first artifact collection — failed

Job: `smartcindy-rust-20261029-02`. Same source/image identity and limits as attempt 2. Release compilation, test-binary compilation with `--no-run`, and rustdoc passed. Artifact collection failed because the command wrote to `/work/source/out` instead of the broker's `/work/out`. This was a probe recipe error, corrected in a new attempt rather than erasing the result.

- Log SHA-256: `77f35525dc51360349e620d2be549a6e5e5b1f6eb6fe39fc090cec621f8876e2`.

### Attempt 4: compile-only and verified artifact return — passed

Job: `smartcindy-rust-20261029-03`. Same source/image identity and limits as attempt 2; corrected artifact path, with test execution explicitly excluded. Native test execution remains a failed capability from attempt 2.

- `cargo check --locked --offline --all-targets`: passed.
- `cargo build --release --locked --offline`: passed.
- `cargo test --locked --offline --all-targets --no-run`: compiled only, no tests executed.
- Rustdoc with warnings denied: passed.
- Release `.rlib`: 3,582 bytes; SHA-256 `c0631824e95fb8961bc73358da20bce7b807f423d93d1f47b1ede4c402d4c1ec`.
- Result archive SHA-256: `1f0d2f859d81c69d576e6122233f544cf48a06cb626233abc84b09182210ff3f`.
- All six returned artifacts were independently rehashed and their sizes checked after the dispatcher verified them.
- Observed cgroups: `cpu.max=200000 100000`, `memory.max=1073741824`, `memory.swap.max=0`, `pids.max=64`.
- Observed workspace: `rw,nosuid,nodev,noexec`, 256 MiB tmpfs.
- Final preflight: no running/managed containers, no active or queued pool jobs, zero CPU/memory reservations, and resource floors satisfied. WSL was started for the postflight and can stop automatically afterward; no persistent uptime is claimed.

Durable [machine-readable evidence](evidence/2026-10-29-danny.json) contains the submitted command list, source snapshot identity, preserved attempt results/logs, final artifact hashes, and postflight. The downloaded binary remains outside tracked source.

## Gates not applicable yet

No business logic exists for meaningful property/mutation tests. No PostgreSQL integration or frontend exists for Testcontainers/browser tests. The current Danny job backend additionally cannot run Testcontainers because it intentionally lacks Docker access, and cannot execute native tests from its noexec workspace. Native build scripts and procedural-macro loading also require an executable-workspace design. These are unimplemented/blocked gates, not skipped passing suites.
