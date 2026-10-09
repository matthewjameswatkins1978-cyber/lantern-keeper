# Lantern Warden Cloud Run deployment

Date: 2026-10-09 (Europe/London)

## Live result

- URL: https://lantern-warden-demo-116252318019.europe-west2.run.app
- Google Cloud project: `resolve-ai-agentic-2608151746` (Resolve AI - Agentic Hackathon)
- Region: `europe-west2` (London)
- Service: `lantern-warden-demo`
- Active revision: `lantern-warden-demo-00002-g2b` (100% traffic)
- Image: `ghcr.io/matthewjameswatkins1978-cyber/lantern-warden@sha256:57f31eec266ec4e6229b7960fcb268573315057b7921d75a6b8ffeea82eba5b8`
- Source/release: commit `c641f211ab4518148270a3589382ced193d788af`, tag `warden-v0.1.1-m7`

## Runtime configuration

- 1 vCPU, 1 GiB memory
- Request-based billing
- Minimum instances 0; maximum instances 1
- Container port 4317; request timeout 60 seconds; max concurrency 20
- Ingress: all; public HTTPS access enabled
- `WARDEN_PUBLIC_DEMO=1`
- No secrets configured

The service is intentionally replay-only. It does not expose public authority administration, Tethers execution, live cognition, provider endpoints, unrestricted AI routes, or a database endpoint.

## Cost protection

The service uses the existing billed project and billing account; no new billing account was created. The available credit is `Marketing - All things agentic hackathon - wturney - 540464243`, with £85.64 remaining at deployment time and an expiry of 2026-11-11.

A monthly, project-and-service-scoped enforced spend cap named `Lantern Warden Cloud Run £5 cap` was created at £4.00, with automatic alerts at £2.00, £3.20, and £4.00. The £1.00 buffer is deliberate. The cap is a protective control, not a mathematical bank-card guarantee: Google billing enforcement can have reporting/in-flight latency, and fixed charges may not be paused by a spend cap.

With min instances 0, max instances 1, request billing, and the Cloud Run free tier, expected hosting cost is negligible for judge-scale traffic. Network egress, storage, other enabled services, and billing latency remain possible charge sources. No artificial traffic was generated to consume credits.

## Verification

External smoke tests against the live HTTPS URL passed:

- `/health`: HTTP 200; `status=ready`, `mode=public-demo`, `live_cognition=false`
- `/console`: HTTP 200; no `NEBIUS_API_KEY`, `TAVILY_API_KEY`, or `LANTERN_TETHERS_AUDIT_TOKEN` content found
- replay sequence: reset -> attack -> denied retry -> grant -> successful retry -> wrong scope -> revoke -> denied retry after revoke
- state reports `mode=REPLAY`, `deployment_mode=public-demo`, `live_cognition=false`, `secrets_included=false`
- security headers present: restrictive CSP, `nosniff`, `DENY`, `no-referrer`, and restrictive permissions policy
- forbidden authority, Tethers, cognitive, dreamer, preview, and `/etc/passwd` paths all returned HTTP 404

Observed resource evidence:

- Cloud Run one-day metrics: memory utilisation 5.5% to 5.99% of the configured 1 GiB, CPU 0.5% to 1.99%, maximum one instance, startup latency about 295–340 ms
- local 1 GiB preflight: 11.14 MiB resident container memory (1.09%), health ready in 1.89 seconds

The public-header hardening was added in commit `c641f21` after the first deployment and redeployed as the active revision above. PR #11 remains open and unmerged; Devpost was not submitted.