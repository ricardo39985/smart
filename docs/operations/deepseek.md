# DeepSeek Flash adapter and internal policy

Verified against official DeepSeek documentation on 2026-10-29:

- [Current model and endpoint](https://api-docs.deepseek.com/quick_start)
- [Chat Completions](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Reasoning controls](https://api-docs.deepseek.com/guides/thinking_mode/)

The server uses `POST https://api.deepseek.com/chat/completions` with model
`deepseek-flash`, currently served by DeepSeek-V4.1-Flash-1017. This is the existing
default text/vision adapter, now extended with explicit reasoning policy. No new
provider-specific product endpoint is required. Provider aliases can change behind
the fixed name; live compatibility is a separate check.

## Development key

On Danny, the trusted operator adds the following to the existing protected
`/opt/smartcindy/dev/secrets/dev.env` (root-owned, mode 600):

```dotenv
DEEPSEEK_API_KEY=YOUR_DEV_KEY
DEEPSEEK_REASONING=none
```

Use the existing private development configuration store for the protected backup.
Never put the real key in `.env.example`, Git, the native app, chat or build inputs.
The executable reads its environment; it does not automatically load a checkout
`.env` file. Missing keys leave paid text admission unavailable. Invalid reasoning
settings fail startup, without including the supplied value in the error output.

The new reasoning environment control requires an updated binary. Coordinate
installation/restart with the UI task after its active native tests finish; this
change does not deploy or restart Danny. Existing runtime/operator boundaries are
in [Danny operations](danny-development-api.md).

## Internal controls

`DEEPSEEK_REASONING` accepts exactly `none`, `low`, `high`, `max`. Omission defaults
to `none`, preserving the current low-latency non-thinking behavior. The adapter
sends matching `thinking.type` and `reasoning_effort` values rather than relying
on the provider's default thinking mode.

Backend orchestration can call `Application::set_text_reasoning(DeepSeekReasoning)`
at runtime. Application clones share one atomic process-wide policy. Each text
request snapshots it immediately before provider submission; in-flight calls keep
their selected policy. This is a trusted Rust control surface, with no public
HTTP configuration endpoint. It resets to configured policy on process restart
and must be applied separately to each process. Background extraction explicitly
keeps reasoning disabled and its existing 2500-token cap.

This groundwork does not implement complexity classification or per-request model
routing. A future concurrent per-request router must carry its selected options
with that request, rather than changing the global default around individual calls.
No automatic paid fallback/resubmission is introduced.

Text output retains the existing 8192 generated-token cap, 64 KiB answer cap and
provider/worker deadlines. Reasoning shares the provider's generated-token budget;
high/max effort may exhaust it or hit the deadline. Such responses fail explicitly
rather than saving a truncated answer or silently increasing the cost limit.
Reasoning content is discarded and is never sent to clients, stored in the
transcript or replayed into later prompts. Cache usage remains normalized as before.

## Native UI handoff

The app continues using `/v1/chats/{id}/turns`, generation SSE/status and capability
discovery. It must not send model IDs, provider names, API keys or reasoning levels;
unknown turn fields remain rejected. No model picker or reasoning selector is
part of the user flow. No Flutter code change is required specifically for this
adapter extension. Native chat wiring remains owned by the UI thread.

Direct cross-thread delivery was attempted through the authorized app tool; it
returned an MCP transport error. This shared document is the durable handoff.

## Verification

The added `server_controls_flash_reasoning_without_client_model_selection` public
journey changes policy through all four levels while a worker remains running,
checks provider protocol options, final answers and private reasoning suppression,
rejects client model/reasoning overrides and checks one paid submission per turn.
It uses real PostgreSQL/pgvector and a local provider protocol simulator.

The focused command compiled but failed before scenario execution at Docker
startup (`Operation not permitted`): 0 passed, 1 setup failure, no skipped tests.
No live key was supplied and no paid request was made. This is not a live model or
reasoning-quality pass. On base `ad72b188414b7ada25be00e6b34bb13e840eba39` plus the
shared uncommitted working tree, these final checks passed:

- `CARGO_BUILD_JOBS=1 rtk cargo clippy --locked --offline --all-targets -- -D warnings`
- `cargo fmt --all -- --check`
- `git diff --check`

Focused runtime command: `CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 rtk cargo test
--locked --offline --test durable_chat_http
server_controls_flash_reasoning_without_client_model_selection -- --exact`.
Mutation testing is not triggered by a new access/deletion/lease rule in this
adapter change; existing ownership/idempotency rules were preserved. Native
acceptance is not required for the internal configuration surface; live provider
compatibility and the existing chat journey remain separate acceptance gates.
