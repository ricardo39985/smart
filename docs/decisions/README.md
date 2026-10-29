# Architecture decisions

Use an ADR for a durable choice affecting public contracts, module/runtime boundaries, storage authority, security, dependencies with substantial operational impact, or testing policy. Routine implementation detail does not need an ADR.

Use the next four-digit number and a descriptive kebab-case name. Include date, status (`proposed`, `accepted`, `superseded`), context, decision, consequences, and verification. Link the replacing ADR rather than rewriting historical reasoning. User approval is needed only when the decision exceeds the authorized task scope.

| Decision | Status |
| --- | --- |
| [0001 Small Cargo foundation](0001-small-cargo-foundation.md) | Accepted for repository foundation; its frontend-in-repository detail is superseded |
| [0002 Flutter client and local checks](0002-flutter-client-and-local-checks.md) | Accepted |
| [0003 Durable native chat API](0003-durable-chat-api.md) | Accepted for this slice; verification pending |
| [0005 Native accounts and explicit identity linking](0005-native-account-authentication.md) | Accepted and implemented; live provider acceptance pending |
| [0006 Verified profile sources and private custom photos](0006-profile-sources.md) | Implemented and verified; live native provider refresh remains pending |
| [0004 Debian native deployment](0004-debian-native-deployment.md) | Accepted; Docker permitted for development only |

Do not describe a proposed decision as implemented. Update relevant contributor docs and status when a decision is applied.
