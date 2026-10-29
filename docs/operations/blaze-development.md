# Blaze development

Blaze development checks work on this backend; builds currently hit upstream compiler limitations. Use the project-scoped, verified indexable-inc/blaze release for diagnostics and keep pinned Cargo for build/test acceptance. The pinned stable release is **0.20261027.0** (commit `49ca152`). Blaze uses its own nightly toolchain dated 2026-10-27, independently of `rust-toolchain.toml`. Its outputs are kept under `.work/blaze-target`, separate from Cargo outputs.

Install and use it from the repository root (the scripts are root-anchored and may be invoked elsewhere):

```sh
sh scripts/install-blaze
sh scripts/dev check --locked
sh scripts/check
```

`sh scripts/dev` defaults to `build`; supported commands are `build`, `check`, `test`, `run`, `status`, and `stop`, followed by normal Blaze arguments. Installation is Linux x86_64 only, pinned to the official release archive and its verified SHA-256. Existing matching installs are retained; installation does not upgrade automatically. Blaze may download its own nightly toolchain into the user's `~/.blaze` cache, separate from the repository pin.

Blaze is for development iteration, not production daemon operation. Its compiler and borrow-checking have upstream limitations. Do not claim compatibility, builds, tests, or speedups without actual verification, and do not silently fall back to Cargo while labeling a result a Blaze pass. `scripts/check` remains the pinned Cargo final correctness verification. There are no GitHub Actions.

## Verified compatibility on 2026-10-29

The SHA-256-verified stable binary reports 0.20261027.0 (49ca152). Installation is project-local, no global Cargo wrapper was enabled, and Rust 1.96.0 remains pinned. The wrapper was invoked by absolute path from /tmp and correctly selected this repository. Script syntax and existing-install idempotency passed.

Blaze check --locked passed on the chat backend, including the final snapshot (0.22s with its daemon already loaded; earlier loads22.16s and21.41s). This is a development diagnostic result, not the pinned toolchain acceptance check. Blaze build on stable failed on the unsupported writable standard-library static __RUST_STD_INTERNAL_VAL. The official candidate 0.20261029.4 was downloaded and checksum-verified separately; its build reached Axum but failed compiling an async handler coroutine layout. No Blaze build/test pass or speedup is claimed. The wrapper supports build/run/test for a later compatible compiler, but those commands are not a working path for this snapshot. Keep the stable pin; do not repeatedly retry these known compiler limitations or rewrite valid application behavior to accommodate them. Recheck a newer reviewed release when compatibility improves.

Cargo.toml uses the documented workspace.metadata.blaze native-proc-macros setting for sqlx-macros: SQLx's async procedural machinery cannot compile as WebAssembly in the tested Blaze versions. This selects native macro expansion while retaining Blaze's macro sandbox; it does not disable all sandboxing. Outputs are separate and no production dependency is added.

Sources: [official installation](https://github.com/indexable-inc/blaze/blob/main/docs/install.md), [usage/toolchain boundaries](https://github.com/indexable-inc/blaze/blob/main/docs/usage.md), [native macro expansion](https://github.com/indexable-inc/blaze/blob/main/docs/cargo.md#proc-macros-run-as-webassembly). Full local logs for this session are under /tmp/smartcindy-blaze-*.log; reproducible facts and limitations are recorded here because temporary logs are not permanent artifacts.
