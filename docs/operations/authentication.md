# Authentication operations

Last configured: 2026-10-29. Development is active on Danny. Production registrations
reserve names/callbacks; no production deployment or public OAuth launch is implied.

## Public registration inventory

| Environment | Google web client | Apple App ID | Apple Services ID | Apple key ID |
| --- | --- | --- | --- | --- |
| Development | 939387336698-j6cftnp8eca3i4j8k00vmt41ui5s50g9.apps.googleusercontent.com | dev.smartcindy.smartcindyApp | app.smartcindy.login.dev | F3SML5ZQ96 |
| Production | 939387336698-fiij9d98hp7rqtt15a9s8nmflhshr4e2.apps.googleusercontent.com | app.smartcindy.mobile | app.smartcindy.login | H677968V98 |

Google project: `smart-cindy`. Apple team: `G58V3W62TQ`. Each provider's return URL is
exactly `https://HOST/v1/auth/PROVIDER/callback`, using `dev.smartcindy.app` or
`smartcindy.app`. Apple domains match those hosts and each Services ID is associated
with its matching primary App ID. Google uses server web clients for the system-browser
flow; no native provider secret or Google client secret is packaged in Flutter.

Google remains external/testing, with the owner registered as a test user. Public
production launch requires branding/publishing review. Do not change other projects/apps.
An initial unused development Google client was deleted with explicit user approval
after its secret appeared in browser output; only its replacement above is active.

## Protected configuration

Owner store: `/home/rick/Dropbox/SmartCindy/secrets/{development,production}/`.
Each contains `google-oauth.json`, its dedicated `apple-signin-KEYID.p8`, and
`authentication.env`, all owner-only. Never print their contents or send them to Pi.
The env keys are `AUTH_ORIGIN`, `AUTH_APP_SCHEME`, `GOOGLE_CLIENT_ID`,
`GOOGLE_CLIENT_SECRET`, `APPLE_CLIENT_ID`, `APPLE_TEAM_ID`, `APPLE_KEY_ID`,
`APPLE_PRIVATE_KEY_FILE`. All provider settings must be complete or absent.

Use exact origin/scheme pairs: development `https://dev.smartcindy.app` /
`smartcindy-dev`; production `https://smartcindy.app` / `smartcindy`.
On Danny, root-only `secrets/dev.env` contains operating configuration. The Apple key
is service-readable `/opt/smartcindy/dev/identity/apple-signin.p8`, mode 0400, in a
root/service-group 0750 directory. Resolve actual UID/GID from the account database;
they are not necessarily equal. Do not weaken the existing root-only env directory.

Only the trusted operator transfers these files. Copy reviewed material with restricted
permissions, stop only the API, explicitly run migrations, restart and verify direct
and public readiness/method discovery. Authentication deployment backup is
`/opt/smartcindy/dev/backups/auth-20261029-retry2`. Earlier failed activations restored
the original binary/env; the final update passed migration/readiness. Migration 0002
is additive and retains existing local sessions. Never restore an old database over
newly created accounts as a routine binary rollback.

`GET /v1/auth/methods` indicates configuration only. It is not a live provider check.
App-identifying HTTPS User-Agent requests reach the public route; a default Python
User-Agent received a Cloudflare 403. No edge security setting was weakened.

## Before production

Complete live Google/Apple native journeys, public Google branding/publishing, verified
email/recovery and erasure, coordinated edge abuse protection, retention/backup recovery,
and native Debian compatibility. Keep provider secrets server-side and sessions only
in platform secure storage. No session in redirect URLs; do not merge identities by email.
