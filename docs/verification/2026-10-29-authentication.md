# Account authentication verification, 2026-10-29

## Passed

On Inspiron, `sh scripts/check` passed formatting, strict all-target Clippy, documentation
checks and 16 tests across seven suites. Real PostgreSQL 18/pgvector Testcontainers
used production migrations. Identity protocol tests used explicitly public synthetic
RSA/EC fixtures and local provider servers; they do not hold deployment authority.

Scenarios cover local signup/profile/logout, Google collision/explicit linking with the
original password still valid, Apple passwordless identity ownership, rejected issuer/
audience/expiry/nonce/algorithm, replay and proof mismatch. Eight completion-proof cases
used fixed seed 73641982. Tests observe public HTTP state rather than private call order.
Four focused mutations (confirmation, reauthentication, native proof and identity ownership)
were caught; no survivor/timeouts in this selected scope. Embedded SQL and all security
rules are not exhaustively mutation-covered.

Two backend regressions were observed before fixes: wrong-provider callback returned
404 rather than contract 400; pending linking survived initiating-session revocation
(returned 200). Unchanged assertions passed after fixing rejection and session binding.

Reviewed binary SHA256:
`c2794e19c0946a5d0f291f7e17795b23b84b6d12c8c72314c043522968922c08`.
Transferred through trusted pinned SSH into Danny Ubuntu-24.04 WSL2; migration 0002,
native API readiness and Google/Apple method discovery passed. Public development
health and method discovery returned 200 with strict TLS. No API runs on Inspiron.

The native Android account journey passed real signup, profile-name edit, sign out and
password sign in through Cloudflare to Danny. The profile edit originally failed with a
TextEditingController used after disposal; the unchanged journey passed after the dialog
owned its controller until route disposal. Android builds/checks ran on Inspiron under
the user's explicit authorization. Danny's constrained runner remains unable to run
Rust/Testcontainers checks; no bypass, replacement runner or Docker deployment was used.

Provider registrations and signing identities are recorded in
[operations](../operations/authentication.md) and the sibling client signing guide.
Both Apple certificates were downloaded by the user after Chrome blocked automated
downloads, matched to their encrypted private keys and converted to encrypted P12 files
inside protected Dropbox. No private signing material is tracked.

## Evidence and limits

OpenAPI 3.1 validation passed using openapi-spec-validator 0.7.2; local documentation links resolve.

Local operator logs (not committed): `/tmp/smartcindy-auth-backend-check.log`,
`/tmp/smartcindy-auth-mutation-retry.log`, `/tmp/smartcindy-app-profile-red.log`,
`/tmp/smartcindy-app-native-account.log`, `/tmp/smartcindy-app-native-final.log`.
These are run evidence, not durable production monitoring.

Live Google/Apple native end-to-end login remains a separate gate. Google authorization reached real owner identity consent and the Danny callback completed with no provider failure (read-only persisted result verified). Return to the native app was not completed in the desktop browser; no native provider acceptance is claimed. Production callbacks/credentials are reserved, not deployed or publicly
published. No macOS/Xcode build, device provisioning, store submission, email verification,
password recovery, account erasure or paid AI-provider compatibility pass is claimed.
