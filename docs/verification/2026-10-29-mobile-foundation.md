# Flutter split and development route — 2026-10-29

The user replaced the browser frontend with one Flutter codebase for Android/iOS in the separate `smartcindy-app` repository and prohibited GitHub Actions. See [ADR 0002](../decisions/0002-flutter-client-and-local-checks.md). No Actions workflows remain in either repository. Historical snapshot evidence remains marked historical.

## Repository and application checks

- Backend: `/home/rick/Documents/Code/smartcindy`, Rust foundation unchanged. The former `apps/web` placeholder and unrun Actions workflow were removed.
- Client: `/home/rick/Documents/Code/smartcindy-app`, existing Git history preserved, Flutter's empty Android/iOS template initialized. No browser/desktop target, business logic, API integration, or provider keys added.
- Pi 1.4.0 / `openai-codex/gpt-6-luna` low reasoning implemented the bounded documentation and Dart/script changes. The primary agent reviewed output, restored detailed backend testing/concurrency invariants where an initial rewrite abbreviated them, and checked the result independently.
- Installed and enforced: Flutter 3.47.0, revision `4cf24164269a5ebf0c16a028a00727d0e77bbb05`, Dart 3.13.0.
- `flutter pub get --offline`: passed; `pubspec.lock` retained for version control.
- The first check script execution exposed an incorrect `flutterRevision` JSON key. The exact installed command emits `frameworkRevision`; that mapping was corrected. The unchanged check entry point then passed all SDK identity checks. This is a scaffold check correction, not a product regression-test claim.
- `dart format --output=none --set-exit-if-changed lib`: passed, one file unchanged.
- `flutter analyze --no-pub`: passed, no issues.
- No behavioral tests exist; `flutter test` was explicitly not run. The generated assertion-free iOS `testExample` was removed while retaining its project file reference.
- Local documentation-link checks and absence of Actions workflow files passed across both repositories.

No Android package, emulator, physical-device, iOS build, signing, publication, Testcontainers, property/mutation, or user-journey acceptance is claimed. Native app IDs and signing remain provisional development configuration.

## Cloudflare route

The user corrected the requested hostname to `dev.smartcindy.app` after the active registered `.app` domain was observed. A dedicated tunnel and proxied DNS route were configured. A mis-scoped record created by the existing zone-bound CLI certificate was removed and verified absent before the correct record was saved in the confirmed zone.

Direct origin and HTTPS both returned the exact temporary marker with status 200 and certificate validation enabled. Both temporary processes were stopped afterward. Automatic approval review initially rejected persistent activation. The user subsequently explicitly approved it: the dedicated user service was installed, enabled, and started; systemd state and four Cloudflare edge connections were verified. A further marker request through that persistent connector returned HTTPS 200. The temporary origin was stopped immediately afterward; the connector remains running, with HTTP 502 expected until the real application server starts.

See [operation details](../operations/development-tunnel.md) and [sanitized HTTPS evidence](evidence/2026-10-29-development-tunnel.json). This proves the route and enabled connector, not a running application backend.
