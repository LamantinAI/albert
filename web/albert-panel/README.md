# Albert panel

React + TypeScript client for the Albert-owned API. Auth, ownership, history,
model selection and configuration application stay on the server.

## Visual system

The workspace uses neutral graphite surfaces, golden ochre accents, local
Space Grotesk text and IBM Plex Mono technical labels. Its dot-matrix paw and
pixel wordmark carry the “Your helping paw” identity. Slow, low-amplitude
brightness changes animate the large identity. Compact pixel branding and the
favicon stay static; a separate paw in chat activity dims its pads during work.
Reduced-motion preferences disable animations. Ordinary interface surfaces
remain flat and neutral.

One authenticated, memory-only session directory supplies sidebar navigation
and conversation creation. The welcome composer hands its first draft to the
new conversation in memory; only acceptance or explicit disposal clears it.
Pagination ignores obsolete responses. Model preference remains workspace-wide,
and all editing, validation, application and approval actions use the typed API.

## Development

Use Node 24.6+ and npm. Dependencies are pinned in `package-lock.json`.

```sh
npm ci
npm run generate
npm run dev
```

The browser app lives under `/ui/`. The development proxy uses
`ALBERT_API_ORIGIN` (default `http://127.0.0.1:9877`). Production uses one origin
and the Octo web connector. Direct HTTP and Host-preserving HTTP development
proxies use automatic same-origin validation; no browser address setting is
needed. TLS termination or rewritten Host headers use an explicit public origin.
The proxy preserves Host and Origin and does not bypass their checks. JWTs remain in an HttpOnly cookie; only the current
CSRF token lives in memory. Browser session storage can contain an account-scoped
pending request ID for receipt recovery, never a draft or transcript.
If the host has exhausted its native file watchers, use
`ALBERT_DEV_POLLING=1 npm run dev`. Browser checks use the production preview
server and do not need file watchers.

## Contracts and verification

Rust Utoipa is the schema authority. `npm run generate` invokes the workspace's
schema-only `xtask` without requiring browser assets, then splits TypeScript
declarations into bounded files. Run it after an API change; do not edit generated
types or maintain a second DTO schema. No OpenAPI JSON snapshot is committed.

```sh
npm run check:api
npm run typecheck
npm test
npm run test:browser -- --project=browser-components
```

Browser component tests use fixtures typed against the generated schema. They
exercise browser behavior and layout; they do not prove backend authentication
or permissions. Set `ALBERT_BROWSER_PATH` to an existing compatible Chromium
executable, or install the project's browser with `npx playwright install chromium`.

The integration project requires a **fresh isolated Albert instance**, temporary
state and a local mock provider; it changes owner credentials and creates data.
Never point it at a live instance.

```sh
ALBERT_E2E_URL=http://127.0.0.1:9877 \
ALBERT_E2E_MODEL_ORIGIN=http://127.0.0.1:18081/v1 \
npm run test:backend
```

## Embedded packaging

```sh
npm run build:embedded
npm run check:embedded
```

This produces disposable `../ui-dist/` assets and a manifest of source and asset SHA256
hashes. The web connector verifies that output, copies it to Cargo's `OUT_DIR` and
embeds it. Cargo never invokes Node or installs packages. The bundle includes
hashed local fonts and their OFL notices, no runtime CDN, source maps or sample
chats. Build these assets before building the Albert application; core and the
schema exporter build independently.

Plain interactive API documentation is embedded at `/docs/`, retaining the
`/ui/docs/index.html` alias linked from Configuration. Its HTML/bootstrap belongs
to `connectors/web/swagger` in the workspace. The exact `swagger-ui-dist` version
is locked in npm; Vite emits its local bundle, default CSS and Apache license.
Swagger uses the current browser cookie and obtains the CSRF token for mutations
from the workspace's API. It uses only that workspace's Utoipa schema, with remote
validation and URL-based configuration disabled.
