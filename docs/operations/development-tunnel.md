# Local development hostname

Confirmed by the user: **`https://dev.smartcindy.app`**. The initially supplied `.com` hostname was corrected to `.app` after checking the registered domain and receiving explicit confirmation.

## Scope and resources

| Resource | Value |
| --- | --- |
| Cloudflare zone | `smartcindy.app`, active |
| Zone ID | `bd00db45e009a580a4c71cdae103f03f` |
| Published hostname | `dev.smartcindy.app` |
| Dedicated tunnel | `smartcindy-dev` |
| Tunnel ID | `e423669e-3764-4c2a-be72-572a4105f0a9` |
| DNS | Proxied CNAME to `e423669e-3764-4c2a-be72-572a4105f0a9.cfargotunnel.com` |
| Local origin | `http://127.0.0.1:8787` |
| Ingress configuration | [ops/cloudflared.dev.yml](../../ops/cloudflared.dev.yml) |
| Enabled user service | [ops/smartcindy-dev-tunnel.service](../../ops/smartcindy-dev-tunnel.service) |

The tunnel credential is outside the repositories under the operator's protected Cloudflare directory. Never copy credential contents into either repository, mobile assets, logs, prompts, or documentation. The local configuration contains only its path. Do not use the general certificate's DNS-routing command for this hostname: the existing certificate is scoped to another zone and may append that zone to the requested name.

Only this hostname and new tunnel were added. Existing tunnels, apex DNS, mail, firewall, TLS policies, and unrelated services were not reconfigured. The unintended `dev.smartcindy.com.socialboard.app` record created during initial routing was matched to the new tunnel, removed, and verified absent through the DNS API before the correct record was added.

## What has been verified

On 2026-10-29:

- Cloudflare dashboard showed `smartcindy.app` active and the correct proxied development record saved.
- Ingress validation and service-file syntax checks passed.
- A time-limited connector established four Cloudflare connections.
- A temporary origin bound only to `127.0.0.1:8787` served a generated, non-sensitive marker on a unique probe path.
- The direct origin and public HTTPS hostname both returned status 200 and the exact expected marker. TLS validation remained enabled. The public response included a Cloudflare Ray ID.
- Python's default user-agent received Cloudflare error 1010. A truthful `SmartCindy-Development-Probe/0.1` user-agent succeeded. No Cloudflare security policy was weakened. Verify the real Flutter client's HTTP behavior when implementing networking; this probe does not prove mobile client compatibility.
- Both temporary processes were explicitly stopped after verification. This was routing evidence, not an implemented chat backend or a continuously running application.

Sanitized verification: [HTTPS route evidence](../verification/evidence/2026-10-29-development-tunnel.json). The temporary endpoint and marker are not an application API contract.

## Persistent activation

**Approved, installed, enabled, and running.** The user explicitly approved persistent activation on 2026-10-29 after the initial automatic-review rejection. The dedicated service was then enabled and started; systemd reported `UnitFileState=enabled`, `ActiveState=active`, and `SubState=running`. Cloudflare reported one connector with four edge connections.

Historical marker verification proved the route before an API existed. Since then, the verified development API runs on Danny's Ubuntu-24.04 WSL2 through the Inspiron SSH origin relay; direct and public API checks passed on 2026-10-29. See [runtime operations](danny-development-api.md) and [runtime evidence](../verification/2026-10-29-development-api.md). The connector alone is not an application-availability guarantee.

The completed activation command and routine state checks are:

```sh
systemctl --user enable --now /home/rick/Documents/Code/smartcindy/ops/smartcindy-dev-tunnel.service
systemctl --user is-enabled smartcindy-dev-tunnel.service
systemctl --user is-active smartcindy-dev-tunnel.service
```

Do not overwrite an unrelated unit with the same name. The connector service starts only cloudflared. The separate SSH origin relay forwards the documented loopback port to Danny's development API. A connected connector without a working relay/API returns a gateway error. Never bind database or admin interfaces to this hostname. The API requires authentication for chat operations; health is intentionally unauthenticated and bounded.

The recorded user approval covers this persistent connector. Do not request the same activation permission again for routine operation within that scope. Broader exposure, different services/ports, credential changes, or unrelated tunnel modifications require their own applicable authorization.

Inspiron user services follow the user's login lifecycle. No Inspiron linger change, system-wide tunnel service, router forwarding, or firewall exception was made. Danny's application services are described separately in the runtime guide. A connected tunnel is not an uptime guarantee.

## Rollback and future changes

Stop/disable only `smartcindy-dev-tunnel.service` if it was activated. Remove only this exact hostname's DNS record and dedicated tunnel when requested; preserve other records and tunnel credentials. Future hostname/port changes require updating this document, the ingress configuration, and the app environment contract together.

No GitHub Actions are involved. Local scripts and reviewed operator commands control development checks and activation.

Reference: [Cloudflare locally managed tunnel setup](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/local-management/create-local-tunnel/). A dedicated local configuration was selected to match this single development host's existing certificate-based workflow without replacing its other tunnels.
