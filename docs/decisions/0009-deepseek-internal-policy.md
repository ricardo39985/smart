# 0009: Default Flash adapter and server-owned reasoning

Date: 2026-10-29. Status: Accepted for the user's DeepSeek groundwork request.

The default hosted text/vision adapter remains DeepSeek's current `deepseek-flash`
alias at its fixed official Chat Completions endpoint. The existing product chat
contract remains provider-neutral. Models and reasoning levels are never accepted
from native users, exposed as selectable options or returned as provider metadata.

Add a typed server policy for none/low/high/max reasoning. Default to none to
preserve current response timing and explicit cost limits. Startup configuration
is an environment value; trusted in-process orchestration may atomically change
the default for future submissions. Background memory extraction retains disabled
reasoning. Shared protocol configuration prevents its model/options drifting from
the foreground adapter.

Complexity-based routing is deferred for discussion. The global default is not a
per-request routing mechanism; future routing must carry decisions with each job
to avoid cross-request policy races. Current generation caps, cancellation fences,
idempotency and no automatic paid retries remain in force.

See [operations, UI handoff and verification](../operations/deepseek.md). Local
provider simulation cannot prove live provider compatibility; Docker access and
an operator-supplied dev key remain separate acceptance prerequisites.
