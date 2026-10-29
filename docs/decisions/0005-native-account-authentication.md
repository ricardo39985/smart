# ADR 0005: Native accounts and explicit identity linking

Status: accepted and implemented, 2026-10-29; real PostgreSQL and simulated-provider checks passed. Live provider and macOS acceptance remain separate gates. Supersedes operator-only accounts for the new signup slice; existing sessions/accounts remain compatible.

## User contract

Google, Apple and email/password are sign-in methods for one person-level account. Identity is `(issuer, subject)`, never email. Never merge existing user rows or move chats based only on matching addresses. A signed-out provider collision offers a warm route back to the original method; linking requires signing in to the original account first, a fresh authentication and explicit confirmation before the provider journey. Existing local credentials remain valid after linking.

External login uses the system authentication browser. The backend owns provider code redemption and client credentials. A five-minute PostgreSQL attempt binds provider, random state, nonce, native S256 proof challenge and optional authenticated linking owner. Only fixed callback URLs and fixed native callback schemes are permitted; no client-supplied redirects. Provider responses are verified for signature, issuer, audience, expiry and nonce. Exchange consumes a one-time result bound to the native verifier; session tokens never appear in callback URLs. Sessions remain opaque, revocable and hashed at rest.

Dev and production use distinct clients and callback origins, exact origin configuration, separate secrets and signing material. Production callback reservation is not production deployment. Signing keys and passwords stay under the owner Dropbox store, with configurable paths for other developers. Preserve the existing development install certificate; production Android gets a dedicated upload identity. Apple identities/profiles are team-managed and require a real macOS/Xcode signing check.

## Acceptance scenarios

- A new local user signs up, signs in, sees their own account, signs out and cannot reuse that session.
- Invalid passwords/unknown users share generic warm advice. Provider-only users are guided to their original method without inventing a password.
- Fresh Google/Apple identities create accounts only after verified provider authentication.
- Same-email collisions never automatically link, even if the provider marks email verified.
- A signed-in local user confirms the linking warning, proves local credentials again, completes provider auth and sees both methods on the same account; cancelling changes nothing.
- Wrong state/nonce/issuer/audience/expiry, unsigned token, reused code/verifier, provider mismatch and expired attempt fail closed. A different account cannot steal an existing identity.
- Authentication work, HTTP bodies, attempts and password hashing remain bounded. Secrets/tokens/passwords are absent from logs, source and mobile assets.
- Account entry opens from every main screen and Back restores the prior screen/draft. One shared circular plus style is used everywhere.
- Appearance supports persisted System/Light/Dark from supplied v3 tokens, responsive text and accessible controls. Unimplemented billing/notifications never report success.
- Native journey exercises local auth against the real development API on Danny; live Google/Apple acceptance is recorded separately from simulated protocol tests. Replacement Dropbox APK is the exact signed tested artifact.

## Backend implementation boundary

Registration and account profile use PostgreSQL users with nullable password hashes; provider subjects are unique per issuer, never derived from email. Proof attempts and one-time completion results are persisted in PostgreSQL with five- and two-minute expiries. The fixed dev/production callback origin and native scheme are validated at startup. Only explicitly configured library-level loopback simulators can replace provider HTTP endpoints; the CLI never enables them. Google and Apple provider keys are cached for one hour with one refresh on an unknown key ID. Signed-out same-email collisions reject rather than merge; explicit linking binds the authenticated owner before redirect. Existing sessions retain their token and response shape. A completion is consumed after the native verifier matches and before account mutation, in one short database transaction. Expired attempts/completions are collected in batches of at most 100 when starting an attempt. A scheduled retention policy for inactive deployments remains an operational gate. Linking also binds the initiating session; revocation invalidates pending linking.

Required repository checks passed against real PostgreSQL/Testcontainers. Four focused identity protocol scenarios and four security mutation checks passed. The Flutter client implements secure sessions and explicit linking. The development API is running on Danny with both providers configured. See [authentication verification](../verification/2026-10-29-authentication.md) for evidence and limits; email verification and recovery remain unimplemented.

## Deferred gates

Email ownership verification/recovery and account erasure need an actual transactional-email/durable deletion integration before production. Do not advertise those as working. Apple private relay addresses are opaque account attributes, not evidence for linking to a different email.
