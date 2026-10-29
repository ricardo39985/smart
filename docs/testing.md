# Testing policy

This is the required guide for deciding **whether, when, and how** to write and run tests. Apply it to human and model-generated code. Its goal is confidence in useful behavior with low maintenance and execution cost.

## Basis and interpretation

Read these before changing the policy:

- [Google: Test Behavior, Not Implementation](https://testing.googleblog.com/2013/08/testing-on-toilet-test-behavior-not.html). Test through public interfaces; an internal refactor with unchanged behavior should normally preserve the assertions. Setup can change when dependencies change. Internal checks are exceptions requiring a meaningful reason.
- [Google: Only Verify State-Changing Method Calls](https://testing.googleblog.com/2017/12/testing-on-toilet-only-verify-state.html). Do not normally verify informational reads or their call order. Assert the result or resulting state instead. Prefer executing real behavior over verifying calls. A narrow interaction check can be justified when the interaction itself is the observable requirement, such as avoiding repeated paid requests.

The following project rules add the user's preferences to that guidance. The articles do not prescribe an E2E-only strategy. This project prioritizes complete user journeys and public behavioral contracts while retaining small tests when they provide cheaper, sharper evidence.

## Decide before writing a test

Write down the behavior and the failure that would matter to a user or API consumer. Choose the smallest **public boundary** that can expose that failure accurately. Cover the complete journey when UI, authentication, persistence, or asynchronous work participates in the requirement.

| Change | Required evidence |
| --- | --- |
| New or changed native user journey | Retain a Flutter integration_test scenario on an Android/iOS device or emulator in the sibling app; browser automation does not establish native acceptance |
| Meaningful fast UI behavior | A focused Flutter widget test through public/user-visible behavior; no widget-tree or private-state assertions |
| Ownership, persistence, migration, jobs, transactions, or concurrency | Public API integration scenario against real PostgreSQL/pgvector created with Testcontainers; add native journey coverage where the user flow warrants it |
| Pure business rule with meaningful cases | Focused examples and/or property tests through its public use-case API; retain journey coverage of the larger flow |
| Bug fix | A failing behavioral regression **before** the fix, then the same test passing after it |
| Refactor preserving behavior | Run existing affected contract/journey tests; do not rewrite assertions to follow the new structure |
| Documentation, empty scaffold, styling with no behavioral change | Relevant build/link/accessibility/visual checks; no manufactured unit tests |
| Dependency or native-runtime change | Relevant integration and compatibility checks; real model check when embedding/runtime changes |

For bugs: establish the reproduction against the old behavior, record the intended failure, make the fix, then run the unchanged regression and affected nearby scenarios. A compile error or unavailable database is not the red regression. If reproduction is impossible, record why and the evidence gap before changing the code; do not label an unobserved failure as reproduced.

## Assert observable outcomes

Name tests for behavior, for example `deleted_memory_is_not_recalled_after_queued_extraction` rather than `calls_delete_then_update`. Prefer HTTP responses, rendered user-visible output, reopened history, notification state, and public read APIs over table-layout assertions.

Use public Rust APIs where a small rule is a legitimate contract. Public APIs do not mean making every function `pub`: keep helpers private and test them through a real caller. Avoid file/module names in scenario boundaries, private imports, snapshots of internal objects, and assertions about helper invocation or read counts.

When persistence or schema behavior is itself the contract, focused storage integration tests may inspect constraints or committed records. Explain that reason; avoid duplicating every repository query test at HTTP and UI layers. Several assertions for one coherent outcome are fine.

Examples:

- A duplicate request produces one saved answer and one settled charge, including after reconnect.
- Another user's chat and attachments cannot be read even with a known identifier.
- A corrected preference is used in the next answer while extraction is pending.
- A cancelled image job displays its actual state without silently submitting a second provider job.

Do not assert the wording of nondeterministic AI output when the requirement is a fact, citation, ownership rule, or lifecycle outcome. Use controlled provider responses for repeatable product assertions and capped live provider checks for actual external compatibility.

## Real dependencies and Testcontainers

Database integration tests must use [Testcontainers for Rust](https://rust.testcontainers.org/) with the production PostgreSQL major, pgvector, and the real migration chain. Pin the reviewed container image digest when adding the harness. Never substitute SQLite or an in-memory repository for PostgreSQL behavior.

Start a disposable container per test binary or isolated suite, then allocate an independent database per scenario. Never share mutable test state or rely on test order. Use container readiness and condition-based bounded waits, not arbitrary sleeps. A Docker failure fails the integration job; do not silently skip it or report zero tests as a pass. Clean up only resources created by that run.

Exercise races with explicit barriers/conditions and bounded deadlines. Use real transactions, multiple connections, and worker takeover. Do not claim race coverage by sequentially calling the same operation twice.

Avoid mocks whenever possible. Use real dependencies and lightweight local HTTP protocol simulators only at unavailable, costly, nondeterministic, or destructive external boundaries. The application must still perform its real parsing, HTTP, streaming, persistence, and orchestration. Keep the simulator independent of private implementation and document what it cannot prove. Use captured synthetic fixtures without customer data.

Do not mock the application layer being tested. Verify a side-effect interaction only if no stronger outcome is observable or if that interaction is the requirement. Do not assert harmless reads or ordering as a proxy for correctness. Live provider smoke checks complement simulators and require test accounts, a stated cost cap, and explicit purpose; never charge real customer accounts.

## Native user journeys and browser surfaces

The Flutter client is in the separate sibling repository `../smartcindy-app`. Native journeys belong under its `integration_test/` directory and use Flutter's [integration_test package](https://docs.flutter.dev/testing/integration-tests). Exercise the real UI on an Android/iOS device or emulator with the Rust backend, disposable PostgreSQL/pgvector, and relevant workers. External paid providers may be locally simulated; label that boundary accurately.

Assert what the user sees or can do: submit a request, receive streamed text, stop/reconnect, reopen durable history, correct a memory, and observe the next answer. Select controls by semantics or intentional stable keys, not private state, widget implementation trees, or layout structure. Focused widget tests are useful for meaningful public interaction states; do not mirror each widget class with a test or add counter-template tests.

Condition-based bounded waits replace arbitrary sleeps. Keep screenshots/device logs for failures using synthetic data. Shared Dart analysis or an Android run does not prove iOS behavior: iOS build and simulator/device acceptance require [macOS and Xcode](https://docs.flutter.dev/deployment/ios).

The user's Playwright MCP preference still applies to actual browser surfaces, for example a hosted checkout or account portal when selected. Use [Playwright MCP](https://github.com/microsoft/playwright-mcp) for browser exploration and retain useful repeatable browser regressions. No browser app target is currently selected. Do not substitute Playwright for native acceptance or claim MCP/device validation when the tools or runtime were unavailable.

## Keep tests simple

Use arrange, act, assert. Test bodies contain explicit inputs, direct actions, and expected outcomes. No branches, ad-hoc retry loops, production algorithm copies, or computed expected results using the code under test. Small data tables are fine; each case must have an independently known expectation.

Helpers may create isolated resources, log in through supported interfaces, or wait for a condition with a deadline. Keep them boring and narrow. Do not create a test framework, hidden assertion DSL, or general fixture factory hierarchy. Property generators are the deliberate exception to fixed inputs, not permission to reproduce business algorithms in the oracle.

## Property and mutation testing

Use [Proptest](https://proptest-rs.github.io/proptest/) for meaningful invariants with broad input spaces: budget conservation, idempotency, ordering, normalization, permissions, memory-generation suppression, and later reminder recurrence. Add properties when such a rule is introduced or materially changed. Define the invariant first and test the public API. Persist minimized regression seeds/cases when a failure is found. Bound cases and runtime; record seed and case count for reproducibility.

Use [cargo-mutants](https://mutants.rs/) to find assertions missing from substantive logic. Run a focused mutation check when introducing/changing critical access, billing, deletion, lease, or ordering rules, and before merging a substantial refactor of those rules. On ordinary edits, target the affected package/files using flags verified against the installed tool's help. Broader runs are manual milestone/pre-release work after relevant logic exists, not automatic work on every push.

Triage surviving mutations: add a behavioral scenario if the mutant exposes a missing requirement; document equivalent/unreachable mutations instead of bending tests around implementation. A timeout/build failure is inconclusive, not a killed mutant. Record scope, results, and unresolved survivors. Do not install and run these tools on a logic-free scaffold to manufacture evidence.

Dart property and mutation tooling will be selected with the first meaningful Dart rules. Do not add packages solely to test this empty app shell. The same invariant-first, bounded-run, and survivor-triage rules apply.

## When and how much to run

1. During implementation, run the smallest relevant behavioral scenario. After a bug fix, rerun its red regression unchanged.
2. Before handing off, run formatting/lint/build plus the affected public contracts and journeys. Run properties/mutations when their triggers above apply.
3. Run repository-local checks manually for the reviewed revision. No GitHub Actions, hosted CI replacement, automatic pushes, or schedules. Add database/native harnesses with real consumers and bound their runtime. Docs-only changes do not launch application suites.
4. Before release, run the complete core journey set, affected failure/recovery scenarios, capped live provider checks, and relevant resource/restore verification. A change to schema, account isolation, memory, or billing warrants broader impact checks than a cosmetic edit.
5. Rerun after a relevant code/configuration change, a failure, or unresolved evidence. Otherwise do not repeat a passing full suite for reassurance.

No required test count, coverage percentage, or one-test-per-function rule. Do not duplicate the same failure at every level unless the levels expose different risks. Remove tests whose only purpose was preserving a removed implementation, while retaining the behavioral contract for retained functionality.

The user's three cost hypotheses are review criteria, not assumed facts: implementation-coupled tests can obstruct refactoring; redundant tests can slow local/build-worker work; generated and repeatedly executed tests have a cost. Judge a test by the regression it can catch, its stability under refactoring, and its execution/maintenance cost. Good small behavioral tests can reduce both debugging time and expensive E2E repetition.

## Current commands and evidence

`sh scripts/check` runs formatting, lint, public HTTP/database contracts, doctests and documentation on Rust 1.96. Docker is required for the Testcontainers PostgreSQL 18/pgvector scenarios. Paid AI and push provider boundaries are locally simulated; this does not establish live compatibility. Target individual scenarios during development with `rtk cargo test --locked --test durable_chat_http <scenario>`, or run the focused `recovery_http`, `memory_http`, `context_http`, `projects_http`, `api_efficiency_http`, `realtime_http`, or `push_http` contract as affected. See `tests/README.md` and the current verification record for exact evidence. Focus mutation scope using the installed tool's current help; do not turn a narrow mutation result into an exhaustive security claim.

The sibling Flutter client still has no chat journey. Its scaffold checks do not establish native product acceptance. Blaze iteration is documented separately in `docs/operations/blaze-development.md`; its compiler limitations and separate toolchain do not replace the pinned Cargo checks.

Every handoff records: behavior changed, exact checks and revision/snapshot, pass/fail/block/not-applicable, environments and dependencies, any red-before-green evidence, and remaining risk. Keep failure logs/seed/trace references. Never confuse compilation, simulation, live provider compatibility, and complete user-flow evidence.
