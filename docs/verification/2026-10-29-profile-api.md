# Profile API verification — 2026-10-29

## Scope and snapshot

This checks Google profile refresh, local-name precedence, permanent account email,
disabled email/password replacement, and private custom photos. It does not claim
live Google/Apple compatibility or native mobile acceptance.

The isolated source directory `/tmp/smartcindy-profile-check` starts from backend
commit `1a692be8158cc9167a0fc545ca1a734ef678b68c`, with the profile implementation,
migration `0003_profile.sql`, profile/account routes, and their HTTP tests. It
intentionally excludes the concurrent memory/context/project work. Tests use real
PostgreSQL 18/pgvector Testcontainers and signed synthetic OIDC responses from a
local HTTP provider simulator. Rust is pinned to 1.96.0.

Snapshot SHA-256 values:

| File | SHA-256 |
| --- | --- |
| `src/identity.rs` | `b637829556f29e6e007281a7b995d2ea5cdd36ddec21b6ac00d37c85cb2ea394` |
| `src/profile_photo.rs` | `260d8607f887264de731145daaac3a8d9b88059c3a022ada139a20a03002eb27` |
| isolated `src/http.rs` | `2f42b663901924b05dce7238a46c9e85efc7eaffa903448ef2b98cdcd89bf421` |
| `migrations/0003_profile.sql` | `4473e8efc9936cda7ae905f0d42e3b2edef37d313d942ef68c68625ecac06332` |
| `tests/identity_http.rs` | `1dee38f2f88048d9f72c99122dc71436852aefea6445e4fd506544fd2825149b` |

The shared identity, photo, and identity-test files matched these isolated files
byte-for-byte after checks. The isolated router contains only the profile slice
on the previously committed router; it is deliberately different from the shared
router containing concurrent features.

## Observed regressions and fixes

- **EXIF orientation:** the public JPEG-upload/PNG-download regression initially
  returned dimensions `(3, 2)` instead of the displayed `(2, 3)`. The decoder now
  reads orientation, retains its bounded output-buffer allocation, applies the
  orientation, and then resizes/re-encodes without metadata. The unchanged test
  passed (6.10 seconds).
- **Replacing a linked Google identity:** selecting another Google subject for
  an account already linked to Google initially returned generic `conflict`
  instead of `identity_already_linked`. A unique-constraint failure also rolled
  back proof consumption. The account's provider subject is now checked while
  holding its user-row lock; rejection preserves the link and consumes the
  confirmation. The unchanged test passed (10.16 seconds), including rejected
  replay and sign-in through the original identity.

The observed red logs are `/tmp/sc-exif-red.log` and `/tmp/sc-link-red.log`.
These temporary files are diagnostic conveniences; the failure descriptions and
retained regression tests are the lasting record.

## Passed checks

Commands ran in the isolated source directory:

```sh
CARGO_TARGET_DIR=/tmp/smartcindy-profile-target cargo test --locked --test identity_http phone_photo_orientation_survives_private_metadata_removal -- --exact
CARGO_TARGET_DIR=/tmp/smartcindy-profile-target cargo test --locked --test identity_http another_google_identity_cannot_replace_link_and_consumes_confirmation -- --exact
CARGO_TARGET_DIR=/tmp/smartcindy-profile-target cargo test --locked --test identity_http -- --test-threads=1
CARGO_TARGET_DIR=/tmp/smartcindy-profile-target CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=1 sh scripts/check
```

RTK wrapped Cargo for compact output. The focused identity suite passed all **17
tests** in 98.52 seconds. The required script passed formatting, strict all-target
Clippy, **29 tests across seven suites** in 227.69 seconds, doctest checking (zero
doctests present), and documentation with warnings denied. No test was skipped.

The photo-size property executes **16 generated cases with Proptest seed
73641990** through authenticated PUT/GET. It checks valid PNG output, dimensions
between one and 512 pixels, no enlargement, a capped longest edge, and aspect
ratio preservation within pixel rounding. Generated cases are not counted as 16
separate Rust test functions.

The email-reservation race starts local signup and verified provider refresh
through HTTP while holding the target address's advisory lock. A bounded
condition waits until both operations reach that barrier before release. The
scenario checks the same provider account remains selected, only one persisted
account owns the normalized address, and another provider cannot automatically
claim it. Database inspection here establishes the cross-table ownership
invariant, rather than asserting internal read order.

The main operator also ran the pinned project Blaze **0.20261027.0** `check
--locked` against the isolated snapshot. It passed in **271.56 seconds**; the
documented stop command released that worker afterward. This is a development
diagnostic result. Pinned Cargo checks above remain the correctness evidence; no
Blaze build/test pass or speedup is claimed.

## Infrastructure failures and recovery

An initial sandbox-only focused run could not bind a loopback listener; rerunning
with the approved local-test permissions passed. An initial parallel identity
suite had 16 disposable-container startup timeouts before application assertions
ran. Serial execution passed all 17 final scenarios; neither failed attempt is
counted as behavioral evidence.

After the required checks passed, a development-binary build exhausted the
temporary filesystem quota. Only this task's build cache was moved to ignored
`.work/profile-check-target` on the SSD, with the original target path retained as
a symlink. This freed 6.3 GB in `/tmp`; the build retry succeeded. Sources and
assertions were unchanged. Subsequent builds use two Cargo jobs, and mutation
caches also live on the SSD.

## Development artifact

```sh
CARGO_TARGET_DIR=/tmp/smartcindy-profile-target CARGO_BUILD_JOBS=2 cargo build --locked --bin smartcindy
```

The original development binary is preserved. A copy was made at
`/tmp/smartcindy-profile-artifact/smartcindy` and processed with
`strip --strip-debug`. It is **35,211,032 bytes**, SHA-256
`83d1bd5f2e8d309e0b1b4aaf3a67c5a234693fab0e381c5b307135fdbcb45e2a`.
`readelf --version-info` reports a highest required GLIBC version of **2.34**, below
Danny's Ubuntu 24.04 GLIBC 2.39. This is a Cargo **development** artifact, not an
optimized production release or a Debian deployment acceptance result.

## Focused mutation checks

**Four selected mutations were caught by the intended public behavioral
assertions: no survivors, timeouts, or unviable mutants.** This is targeted
evidence, not an exhaustive security or mutation-coverage claim.

| Mutation | Public regression | Observed failure |
| --- | --- | --- |
| Replace the local-name source condition with an always-true bound expression | `google_profile_refresh_preserves_local_edits_and_custom_photo` | `Maya Updated` replaced the expected `Maya at home` |
| Remove avatar-read ownership while retaining the bound user parameter | `private_avatar_replacement_and_deletion_only_affect_owner` | A different user received 200 instead of 404 |
| Remove avatar-delete ownership while retaining the bound user parameter | `private_avatar_replacement_and_deletion_only_affect_owner` | A different user's deletion cleared the owner's custom-photo flag |
| Reverse `linked != subject` in `refresh_identity` | `another_google_identity_cannot_replace_link_and_consumes_confirmation` | Generic `conflict` replaced the expected `identity_already_linked` |

The three SQL mutants ran serially in `/tmp/smartcindy-profile-mutations`, using
the SSD target `.work/profile-mutation-target` and the following targeted command
after each single mutation. Every source change was restored in a `finally`
block. SQL literals need these explicit mutations because cargo-mutants does not
generate mutations inside SQL strings.

```sh
CARGO_TARGET_DIR=/home/rick/Documents/Code/smartcindy/.work/profile-mutation-target CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=1 cargo test --locked --test identity_http SCENARIO -- --exact
```

The generated guard mutation used **cargo-mutants 27.1.0** after inspecting its
installed help/list output:

```sh
CARGO_TARGET_DIR=/home/rick/Documents/Code/smartcindy/.work/profile-mutation-target CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=1 cargo mutants --in-place --baseline skip --timeout 120 --build-timeout 180 --no-config --file src/identity.rs --re 'replace != with == in refresh_identity' --output /tmp/smartcindy-profile-generated-mutation -- --test identity_http another_google_identity_cannot_replace_link_and_consumes_confirmation
```

Here `--in-place` refers only to the separate mutation copy, never the shared
checkout or artifact source. The unchanged snapshot had already passed the full
baseline, so it was not repeated. Cargo-mutants reported **one caught mutant in
65 seconds** (successful compilation, then the expected assertion failure).
The mutation copy's identity and photo sources were restored and compared with
the validated source afterward. The deployable artifact remains separate.

Temporary detailed evidence is under
`/tmp/smartcindy-profile-sql-mutation-logs/` (three logs plus `results.json`) and
`/tmp/smartcindy-profile-generated-mutation/mutants.out/` (diff, log, and
`outcomes.json`). The cases, observed failures, and retained tests above provide
the lasting record if temporary files are removed.

## Separate acceptance gates

Development deployment, authenticated native photo selection/display, live Google
profile refresh, Apple sign-in, and iOS/macOS acceptance remain separate results.
The main operator may append deployment/native evidence here after verifying it.

## Development runtime verification

The operator deployed the exact artifact above to Danny's existing native Ubuntu
24.04 WSL2 development service. Migration 0003 and direct health passed. Only the
Smart Cindy API restarted; its prior binary was retained for rollback. PostgreSQL,
configuration, signing keys and tunnel services were preserved.

Public TLS checks through Cloudflare returned 200 for `/healthz` and
`/v1/auth/methods` (password/Google/Apple enabled), and 401 for an unauthenticated
`/v1/account/avatar` read. The Cloudflare Ray header was present. This is the
profile-only baseline snapshot; concurrent memory/context/notification work was
not included. Native photo evidence is recorded in the sibling client repository.
