# Rust Pan Link Checker API Design

Date: 2026-05-25

## Goal

Create a Rust service foundation for checking whether cloud drive links are valid. The service must support both direct HTTP API calls and Telegram bot webhook calls.

## Scope

- Provide a reusable HTTP API for link checking.
- Provide a Telegram webhook entry that accepts bot updates and replies with check results.
- Keep link checking logic independent from Telegram handling.
- Add an extensible checker boundary for future provider-specific and browser-based checks.
- Preserve a clean path for adding a Web UI later without rewriting the API or checker.

## Non-goals

- Full support for every cloud drive provider in the first version.
- Production deployment automation.
- Persistent database storage.
- Running a browser process inside the first version.
- Building the Web UI in the first version.

## Architecture

The service will use Axum as the HTTP server and Teloxide for Telegram types/client integration.

Main modules:

- `config`: load runtime settings from environment variables.
- `routes`: register health, API, and Telegram webhook routes.
- `checker`: define the link checking interface, request model, response model, and first generic implementation.
- `providers`: contain provider-specific modules such as `pan115`, `pan189`, and `pan123`.
- `telegram`: parse Telegram updates, extract `/check <url>` commands, call the checker, and send replies.
- `error`: centralize API error responses.

The checker will expose one application-level method:

```rust
async fn check(&self, request: CheckRequest) -> Result<CheckResult, CheckError>;
```

This keeps HTTP routes, Telegram routes, and future workers dependent on the same abstraction.

Initial Rust module layout:

```text
src/
  main.rs
  app.rs
  config.rs
  error.rs
  routes/
    mod.rs
    api.rs
    health.rs
    telegram.rs
  checker/
    mod.rs
    model.rs
    service.rs
    batch.rs
  providers/
    mod.rs
    common.rs
    generic.rs
    pan115.rs
    pan189.rs
    pan123.rs
  telegram/
    mod.rs
    handler.rs
```

The first version will keep these as modules in one crate. This follows KISS and avoids premature workspace complexity. If individual provider implementations grow large, they can later be split into separate crates without changing the checker interface.

Provider modules should implement a shared provider strategy boundary:

```rust
trait ProviderChecker {
    fn provider(&self) -> Provider;
    fn matches(&self, url: &Url) -> bool;
    async fn check(&self, context: CheckContext) -> Result<CheckResult, CheckError>;
}
```

The provider registry selects the first matching provider module. If no provider matches, it falls back to `generic`.

Web UI compatibility constraints:

- Keep all machine-facing routes under `/api/*`.
- Keep Telegram-only routes under `/telegram/*`.
- Keep response models stable and JSON-first so a future browser UI can call the same API.
- Add CORS as configurable middleware when the UI is hosted separately.
- Reserve static asset serving or SPA fallback as an application-layer option, not part of checker logic.

## HTTP API

Endpoints:

- `GET /healthz`
  - Returns service health.
- `POST /api/check`
  - Request body: `{ "url": "https://..." }`
  - Response body includes normalized URL, status, detected provider, reason, and optional metadata.
- `POST /telegram/webhook`
  - Receives Telegram update payloads.
  - Requires a Telegram bot token to send replies.
  - Optionally checks a webhook secret header if configured.

Future Web UI routes should not overlap with `/api/*` or `/telegram/*`. A same-binary deployment can later serve assets from `/` while continuing to expose the API under `/api`.

## Link Checking Behavior

First version behavior:

- Validate URL syntax.
- Reject unsupported schemes except `http` and `https`.
- Detect provider from hostname using a small provider classifier.
- Perform a bounded HTTP request with timeout and redirect handling.
- Return a structured status:
  - `valid`
  - `invalid`
  - `unknown`

Provider-specific implementations can later refine status by parsing page content, API responses, or browser-rendered state.

Initial provider modules:

- `pan115`: reserved for 115 share links and future extraction-code behavior.
- `pan189`: reserved for China Telecom Cloud share links.
- `pan123`: reserved for 123 Pan share links.
- `generic`: fallback for unknown providers and basic HTTP evidence.

Each provider module owns its own host matching, page signal parsing, and optional browser detection rules.

## Browser Scraping Extension

Browser scraping will be modeled as an optional adapter behind the checker boundary. The initial code will not start Selenium or Chrome by default.

Future browser-backed checker responsibilities:

- Open links through WebDriver.
- Wait for stable page state.
- Extract provider-specific invalid/expired/password-required signals.
- Return the same `CheckResult` model as non-browser checkers.

## Future Web UI Extension

The first version will not include a UI, but the backend should remain UI-ready:

- API responses should include enough structured fields for display, filtering, and status badges.
- Error responses should use stable codes so the UI can map them to localized messages later.
- Link checking should stay stateless at the route level, allowing a future UI to add history or background jobs without changing the initial check endpoint.
- The service can later add static file serving, an SPA fallback, or a separate frontend origin with CORS.

## Batch Detection Efficiency

Batch checking should use the same provider modules as single-link checking.

First version batch behavior:

- Normalize and deduplicate URLs before outbound requests.
- Reuse one HTTP client with connection pooling.
- Use configurable global concurrency and per-host concurrency.
- Read only the data needed for status signals.
- Treat browser detection as a later selective fallback, not the default path.

Future high-volume behavior can add async jobs:

- `POST /api/jobs/checks`
- `GET /api/jobs/{id}`

## Configuration

Environment variables:

- `APP_HOST`: bind host, default `127.0.0.1`.
- `APP_PORT`: bind port, default `8080`.
- `TELOXIDE_TOKEN`: Telegram bot token. Required for Telegram replies.
- `TELEGRAM_WEBHOOK_SECRET`: optional Telegram webhook secret validation.
- `CHECK_TIMEOUT_SECS`: outbound check timeout, default `10`.

## Error Handling

API errors return JSON with a stable error code and message. Telegram errors are logged and converted into user-readable replies when possible.

The checker should avoid panics on malformed input, network timeouts, redirect loops, DNS failures, or unsupported schemes.

## Testing

Initial verification should include:

- Rust formatting check.
- Rust compilation check.
- Unit tests for URL validation and provider detection.
- Route-level tests for `/healthz` and `/api/check` malformed input.
- A simple assertion that API route paths stay under `/api` and Telegram routes stay under `/telegram`, preserving future UI routing space.

## Open Decisions

- Exact list of first-class provider modules can be expanded after the foundation is in place.
- Browser automation backend can be selected later; `thirtyfour` is the current default candidate because it integrates with standard WebDriver.
