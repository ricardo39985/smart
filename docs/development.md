# Development workflow

Read [architecture](architecture.md), [testing](testing.md), and [status](status.md) first. The repository contains the first durable chat backend slice; the separate Flutter client remains a shell. See current status for verification and release gates.

## Start and finish a change

1. Inspect current files and Git status. Preserve existing user changes. Read applicable `AGENTS.md` instructions.
2. Write the user-visible behavior, public boundary, acceptance scenarios, and out-of-scope changes before implementation. Resolve ambiguity before assigning a low-complexity coding task.
3. Select the smallest coherent implementation. Check current primary documentation for actual integration details. Add dependencies only with a real consumer and inspect default features, compatibility, maintenance, and licensing.
4. Follow the testing policy. Reproduce a bug red before fixing it. Avoid rewriting test expectations to match an internal refactor.
5. Review the diff, run relevant checks, update affected docs and actual status, then report concrete evidence and remaining gates. Do not deploy or publish as part of ordinary local coding.

## Pi coding and model routing

Pi is the preferred coding tool. The main agent/developer owns architecture, acceptance criteria, model selection, review, and verification. Model-produced changes are not self-approving.

| Task | Routing |
| --- | --- |
| Fully specified scaffolding, routine adapter, bounded mechanical change | Available Luna model, low reasoning initially |
| Several interacting behaviors with clear contracts | Luna if acceptance is explicit; increase reasoning or route to a stronger available model when dependencies remain difficult |
| Ambiguous architecture, security, billing races, data loss, memory correctness | Stronger reasoning for design/review; delegate clear implementation slices afterward |
| Failed acceptance or scope uncertainty | Diagnose the failure; escalate when reasoning is the bottleneck rather than repeatedly retrying the same prompt |

Do not infer price/performance from a model name. Confirm availability using `pi --list-models luna`. The foundation session verified `openai-codex/gpt-6-luna` and Pi 1.4.0. Future agents must recheck availability rather than silently substituting another model.

Check authentication without revealing credentials:

```sh
pi auth check --provider openai-codex --model gpt-6-luna --no-refresh --json
```

Never use credential-printing commands or put API keys into command arguments. Pi needs normal access to its own configuration lock files; a sandbox permission failure is not an authentication failure. Do not copy auth files into the repository to bypass it.

For a precisely specified file-editing task, a minimal invocation is:

```sh
pi --provider openai-codex --model gpt-6-luna --thinking low \
  --no-extensions --no-mcp --no-skills --no-prompt-templates \
  --no-session --tools read,edit,write -p @/absolute/path/to/task.md
```

Review project-local Pi resources before enabling them with its approval option if required. Do not enable unrestricted shell, third-party extensions, remote host control, or credential access for a task that only needs edits. Use project-root paths and explicitly listed output files. Grant additional tooling only for a concrete purpose. Existing authorization and the current environment's permissions determine whether an action can run.

Each task brief must include:

- Goal and one observable acceptance contract.
- Allowed files, required context, and excluded scope.
- Concrete inputs/outputs and failure behavior.
- Relevant architecture/ADR/testing rules.
- Verification commands and dependencies; who will run them.
- A stop condition for unresolved design or unavailable prerequisites.

Keep prompts/results outside tracked source unless they are useful engineering records. Do not send credential directories, production data, or unrelated projects as context.

## Local commands

Install the pinned Rust toolchain using the usual rustup workflow if absent. The foundation uses Rust 1.96.0 with rustfmt and Clippy. Track `Cargo.lock` even before dependencies are introduced.

```sh
sh scripts/check
rtk cargo test --locked
```

Use RTK for human-facing output. For JSON consumed by tooling, use raw Cargo output. The sibling Flutter client is `../smartcindy-app`; use its documented `sh scripts/check` for local checks. Resolve packages with `flutter pub get` when needed and track `pubspec.lock`. No hosted CI is used; local checks and deployment are manual and reviewed. The Flutter client has no chat UI yet. Backend startup and the actual HTTP contract are in docs/api.md and README.md.

## Blaze development iteration

Use the project-scoped pinned Blaze toolchain for development iteration as documented in [Blaze development operations](operations/blaze-development.md). Its stable version and independent nightly toolchain do not alter the repository's Rust pin. Keep Blaze outputs separate from Cargo outputs. Blaze has upstream compiler and borrow-checking limitations; Cargo through `sh scripts/check` remains final correctness authority. Do not claim unverified builds, tests, or speedups, or report a Cargo fallback as a Blaze pass.

## Dependency and debt control

Keep `Cargo.lock` current. Do not use wildcard dependencies, unpinned Git revisions, or future package layers. An added dependency must remove necessary machinery rather than hide unclear requirements. Pin native model/runtime identities when the embedding slice arrives.

Prefer explicit code and concrete types. Avoid a framework within the application, broad generic interfaces, redundant DTO copies, and configuration switches without a supported behavior. Introduce capability modules as needed; split large modules when a coherent boundary emerges, not at an arbitrary line count.

Document durable decisions in ADRs; keep live implementation facts in `status.md`. Temporary limitations need a concrete next acceptance gate, not an unexplained TODO. Check scripts and operator workflows must describe only commands that exist. Do not add GitHub Actions or silently substitute another hosted build service.

## Danny workhorse

Use [the bounded build dispatcher](operations/danny-build.md). The primary trusted operator controls submission; Pi and other coding models receive no host credentials. The current networkless worker boundary does not support Testcontainers because jobs deliberately have no Docker socket. Its workspace also prevents native executable test runs. Use it for reviewed compilation jobs; run behavioral and database integration tests on a suitable approved development host until a reviewed backend supports them. Never weaken its mounts or expose the Docker socket as a workaround.

The local development route is documented in [development tunnel operations](operations/development-tunnel.md). It has its own connector lifecycle and approval state; GitHub repository hosting does not host the backend.

## Debian production boundary

[ADR 0004](decisions/0004-debian-native-deployment.md) selects native Debian services on Lightsail. Docker is allowed for development/builds and remains required by the existing PostgreSQL Testcontainers suites; it is not a production runtime requirement. Produce a versioned native Rust artifact against a compatible Debian userspace and verify it on the actual deployment release/architecture. Do not replace real database tests because production has no Docker. Danny remains the intended workhorse, subject to the documented runner restrictions and reviewed setup.
