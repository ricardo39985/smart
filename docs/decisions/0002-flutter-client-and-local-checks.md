# 0002: Separate Flutter client and manual local checks

Date: 2026-10-29

Status: Accepted.

## Context

The user selected a native Android/iOS Flutter client while retaining a Rust-only backend. The prior web frontend and GitHub Actions choices are superseded.

## Decision

Keep backend Rust code in this repository and the Flutter client in the separate sibling repository `../smartcindy-app`. Shared Dart code belongs under `lib/`; platform glue stays minimal under `android/` and `ios/`. Repositories own separate manifests and lockfiles. Backend API contracts are authoritative; generate Dart types when an actual API contract exists. Do not duplicate business rules or create speculative OpenAPI/contracts code.

Use repository-local manual check scripts and reviewed deployment. Do not add GitHub Actions or another hosted CI service, automatic pushes, or schedules. Native journeys use Flutter integration_test on devices/emulators; browser automation is only for real browser surfaces. Before native billing, review applicable distribution policy.

## Consequences

There is no browser or desktop Flutter target. No native auth/session, secure credential storage, durable-stream reconnect/cancel, or FCM/APNs implementation is claimed; define those contracts when implemented. Backend status and the client status must distinguish planned behavior from working behavior.

## Verification

The app shell and local check script are scaffold only. Offline package resolution, strict SDK version/revision checks, formatting, and analysis passed after independent review. There are no behavioral tests yet. iOS verification requires macOS and Xcode and has not run. No hosted CI is configured.
