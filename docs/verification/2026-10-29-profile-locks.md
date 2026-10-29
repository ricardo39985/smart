# Profile controls and disabled credential changes

Scope: the user's appended backend profile requirement, 2026-10-29. Existing
concurrent name/provider-photo/private-avatar work was preserved. This record
does not claim that work was freshly executed against PostgreSQL here.

## Behavior

- `GET /v1/account` always returns the canonical `users.email`, including after
  a signed Google/Apple identity presents a changed email claim.
- The account response exposes `capabilities.email_change=false` and
  `capabilities.password_change=false`.
- Authenticated email/password change routes return `503` with
  `email_delivery_unavailable`; no credential replacement, addition or email
  delivery is attempted. Anonymous requests require authentication.
- Name edits and private profile photos remain supported. The photo is stored
  independently of chat attachments; deleting a chat does not delete it.

## Checks

**Passed:** Rust formatting; `rtk cargo check --offline --locked --test
identity_http`; diff whitespace validation. A transient warning from concurrently
unwired memory indexing was unrelated to profile compilation.

**Blocked:** execution of the real PostgreSQL/Testcontainers journeys and focused
mutation checks, because the current environment denies Docker socket access.
No substitute database, skipped tests or mocked application layer was used.
The changed locked-email expectations were not observed failing before the change;
this is a new user-requested contract, not a claimed red-to-green regression.

**Not applicable:** live provider charges, native-client acceptance, deployment,
email delivery and password recovery. No such operations were performed.

The public HTTP scenarios cover unchanged canonical email after provider refresh,
local name/custom-photo preservation, profile ownership, profile lifetime after
chat deletion, disabled email/password changes for both local and provider-only
accounts, anonymous rejection and the original password remaining valid.

## Subsequent coordinated verification

The profile task subsequently included this disabled-boundary contract in its
isolated baseline plus profile snapshot. All 17 identity scenarios, the full
29-test check and four focused mutations passed with approved local Docker access.
See [profile API verification](2026-10-29-profile-api.md) for exact evidence and
Danny deployment. The blocked record above describes the earlier task's environment.
