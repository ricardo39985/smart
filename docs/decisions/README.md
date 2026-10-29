# Architecture decisions

Use an ADR for a durable choice affecting public contracts, module/runtime boundaries, storage authority, security, dependencies with substantial operational impact, or testing policy. Routine implementation detail does not need an ADR.

Use the next four-digit number and a descriptive kebab-case name. Include date, status (`proposed`, `accepted`, `superseded`), context, decision, consequences, and verification. Link the replacing ADR rather than rewriting historical reasoning. User approval is needed only when the decision exceeds the authorized task scope.

| Decision | Status |
| --- | --- |
| [0011 Local development runtime; Danny remains a workhorse](0011-local-development-runtime.md) | Accepted and activated |
| [0001 Small Cargo foundation](0001-small-cargo-foundation.md) | Accepted for repository foundation; its frontend-in-repository detail is superseded |
| [0002 Flutter client and local checks](0002-flutter-client-and-local-checks.md) | Accepted |
| [0003 Durable native chat API](0003-durable-chat-api.md) | Accepted for this slice; verification pending |
| [0005 Native accounts and explicit identity linking](0005-native-account-authentication.md) | Accepted and implemented; live provider acceptance pending |
| [0006 Verified profile sources and private custom photos](0006-profile-sources.md) | Backend verified and deployed to development; native acceptance in progress |
| [0007 Memory, context and cache accounting](0007-memory-context.md) | Implemented; database/live-model verification pending |
| [0008 Native notifications and lightweight invalidation](0008-notifications.md) | Implemented foundation; provider/native acceptance pending |
| [0009 DeepSeek Flash and internal reasoning policy](0009-deepseek-internal-policy.md) | Implemented; database/live-provider acceptance pending |
| [0010 Chat-head generation lookup and title search](0010-chat-head-lookup-and-title-search.md) | Accepted |
| [0004 Debian native deployment](0004-debian-native-deployment.md) | Accepted; Docker permitted for development only |

Do not describe a proposed decision as implemented. Update relevant contributor docs and status when a decision is applied.
