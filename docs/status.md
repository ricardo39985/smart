# Current implementation status

Last updated: 2026-10-29.

## Profile refinement (2026-10-29)

Verified Google name/photo claims refresh after successful sign-in or explicit
confirmation of the same linked identity. Local names and custom photos retain
precedence. Account email stays fixed, and email/password replacement is explicitly
unavailable until verified delivery exists. Private native photo uploads are
authenticated, oriented, bounded, resized and stripped of metadata.

The isolated baseline plus profile migration 0003 passed 29 public contracts across
seven suites, strict Clippy, formatting and documentation. Its development binary
is active on Danny; public Cloudflare health is 200 and unauthenticated avatar reads
are 401. See [profile verification](verification/2026-10-29-profile-api.md). This
profile deployment does not include the separate memory/notification working tree.

## Deployment decision

Production now targets native services on Debian Lightsail, with no Docker or Compose deployment. Docker is permitted for development/builds and retained for the real PostgreSQL/pgvector Testcontainers suites. See [ADR 0004](decisions/0004-debian-native-deployment.md). The exact Debian release/architecture and native compatibility are release checks; no cloud instance, host service, runner or deployed application was changed by this decision. The FreeBSD proposal was withdrawn.

## Implemented and locally verified

The Rust backend now implements local signup, editable account names, Google/Apple identity authentication with explicit linking, and revocable native bearer sessions;
chat create/list/read/rename/archive/delete; bounded cursor pagination; immutable message
editing, regeneration and branches; private validated image upload/download and image
editing/generation; durable request idempotency, queued/fenced workers, cancellation,
status and reconnectable SSE. The public API accepts product concepts and attachment IDs,
not provider/model names or arbitrary remote image URLs. Usage counts are normalized.

Chat, message and branch lists use bounded pages and stable creation/branch cursors.
New replies do not replace the ancestry selected by an older-page cursor. Branch listings
retain the heads that existed at their initial watermark even if those branches continue.
Chat deletion cancels active work and purges images exclusive to the deleted references,
reclaiming quota atomically while preserving shared live images. Live SSE bodies have a
32-subscriber bound per Application/process; disconnect returns capacity.

The [API guide](api.md), [OpenAPI 3.1 schema](openapi.json), and [ADR 0003](decisions/0003-durable-chat-api.md)
are the engineering contracts. The CLI supports direct migration, protected-input account
provisioning and serving on 127.0.0.1:8787. Runtime connection pooling is separate from
migration connections. On 2026-10-29 the development API and PostgreSQL were migrated/started
as native services on Danny's existing Ubuntu-24.04 WSL2, and direct plus public HTTP behavior
was verified. This is not production deployment. See [runtime verification](verification/2026-10-29-development-api.md)
and [operator guide](operations/danny-development-api.md).

## Verification results

- **Passed on Danny:** native API/PostgreSQL startup, real migrations, direct readiness and
  public HTTPS readiness; authenticated session and chat lifecycle smoke through Cloudflare,
  including revoked-session rejection. Synthetic data was removed. See [runtime evidence](verification/2026-10-29-development-api.md).
- **Passed on Inspiron for the readiness addition:** formatting, all-target strict Clippy,
  locked API binary build, the focused real-PostgreSQL readiness HTTP scenario and valid
  OpenAPI 3.1 (15 paths). The earlier backend scenario evidence below was not rerun for this isolated route addition.
- **Passed:** formatting, strict Clippy, all 10 public HTTP integration scenarios against
  disposable PostgreSQL 18/pgvector and production migrations; documentation/doctest checks.
  The suite includes 24 public cursor/limit property cases with seed 73641981. The final
  additional unknown-field validation assertion passed in its affected HTTP scenario;
  formatting/lint were checked afterward.
- **Passed:** three observed red-to-green regressions: branch pages losing a continued head,
  last-chat deletion leaving images undeletable, and unlimited SSE subscribers. Assertions
  were retained through each fix. Concurrency/recovery cases include simultaneous duplicate
  acceptance, cancelled/deleted work, durable history/replay, and expired running jobs
  failing without paid resubmission. These are focused cases, not an exhaustive stress audit.
- **Passed:** four focused mutation checks of session expiry, cursor owner/chat scoping and
  page bounds; four caught, no survivors, timeouts or unviable mutants in that selected scope.
  Those guards did not change afterward. Embedded SQL and all security rules are not covered
  by this narrow mutation result.
- **Passed:** OpenAPI 3.1 validator, JSON/reference resolution and route-path coverage checks.
  Official DeepSeek and BFL contracts inspected; local paid-provider protocols simulate
  streaming, vision, generation/editing and failures. Simulation is not live compatibility.
- **Blaze:** requested development tool installed/pinned separately. Stable development
  check passed, with Cargo 1.96 retained as final authority. Stable build and a separately
  verified current candidate build hit upstream compiler limitations; no Blaze build/test
  pass or speedup is claimed. See [Blaze operations](operations/blaze-development.md).

Evidence, exact commands, snapshot hashes and limitations are in
[backend verification](verification/2026-10-29-chat-backend.md).

## Remaining acceptance and release work

Live paid-provider text/vision/image generation/editing checks need protected credentials
and a capped test budget. No live requests or charges were made. Deployment, coordinated
edge abuse limits, PgBouncer settings, capacity measurement, backup/restore, account erasure,
orphan/transcript retention and unknown-paid-outcome reconciliation remain release work.
Current context limits are explicit; memory compaction is not silently substituted.

The sibling Flutter app now has Chat, Your space and Create preview screens, lazy sample
library rows, retained local drafts and a real development health consumer. Its 21 local
checks and native Android shell/readiness journey passed on Inspiron under the user's
explicit local-Android authorization. It builds without private files or this checkout.
Authenticated live chat pagination, stable IDs, anchored older-page loading, per-branch
cursors, bounded history/image caches, streaming replay remain client
work. Secure account sessions and local login now work; see the authentication verification below. iOS/macOS/Xcode compilation and native chat acceptance remain pending. See the
[client status](https://github.com/ricardo39985/smartcindy-app/blob/main/docs/status.md).
No browser frontend or Playwright acceptance is claimed.

Email verification/recovery, account erasure, subscription billing/distribution review, Rig/rig-memory/mem0-rs,
personal memory/embeddings, research/search and reminders/push remain separate product
slices. Selected memory libraries are deferred, not replaced by another transcript store.

The development route now reaches the running Danny development API through the existing
Inspiron connector and SSH relay. Both machines, LAN, user services and awake Windows are
required; this is not an unattended availability guarantee. Danny's bounded build worker is
unchanged and still cannot execute native tests/Testcontainers; no mount/socket workaround was
used. The API binary was built on Inspiron, not Danny. No GitHub Actions or replacement hosted
CI was introduced.

## Authentication and signing slice (2026-10-29)

The development API on Danny now has local account creation/name editing and Google/Apple
provider configuration. The mobile app uses secure device sessions, a system-browser provider
flow, explicit linking and persisted appearance. Stable Android development/upload signing
identities and Apple development/distribution certificates are stored privately in Dropbox.
No production host, store release or hosted CI was created.

Passed: required Rust checks (16 tests across seven suites), real PostgreSQL identity
contracts, eight seeded completion-proof cases, four focused mutations caught, and the
native Android local signup/edit/logout/login journey through Cloudflare to Danny.
The profile-dialog disposal bug and logout/pending-link vulnerability were reproduced
before fixes, with unchanged regressions passing afterward.

Live Google/Apple user journeys, public Google OAuth publishing/branding, macOS/Xcode
signing/device verification, email verification/recovery, abuse controls at the edge and
account deletion remain acceptance/release gates. Production provider registrations reserve
callbacks only. [Evidence and limitations](verification/2026-10-29-authentication.md).
