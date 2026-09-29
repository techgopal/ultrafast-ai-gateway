# Console, Plan 1: Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A web console, compiled into the gateway binary, where a user signs in and manages users, teams, virtual keys, providers, access tokens and their own account, and an admin reads the audit log.

**Architecture:** A static single-page app under `ui/` (React, TypeScript, Vite, TanStack Router, Query, Table and Form). Its built files are embedded in the `ultrafast` binary and served at `/`, with the existing `/api`, `/v1` and `/health` untouched. The app talks only to `/api`, through a client whose types are generated from `openapi/admin.json`.

**Tech Stack:** TypeScript, React, Vite, TanStack Router / Query / Table / Form, Vitest, Testing Library, MSW, Playwright, pnpm. Gateway side: rust-embed, mime_guess.

**Spec:** `docs/superpowers/specs/2026-09-28-gateway-v2-design.md` (section 13, and sections 6 and 8 for what the API enforces)
**Prototype:** https://claude.ai/artifact/UaWzRPnuMzfVVrZkFSceAk (layout, navigation and visual language; its numbers are sample data)

## Where this plan sits

The console is built in steps that follow the backend:

| Plan | Delivers | Needs |
|---|---|---|
| **Console 1 (this plan)** | Shell, sign-in, setup, invites, users and teams, virtual keys, providers, access tokens, account, audit log, overview of what exists | Gateway plans 1 and 2 (done) |
| Console 2 | Models, routing | Gateway plan 3 |
| Console 3 | Budgets and limits | Gateway plan 4 |
| Console 4 | Logs, overview charts, playground | Gateway plan 5 |

Pages whose backend does not exist yet (Logs, Playground, Models, Routing, Budgets and limits, Guardrails, MCP tools) appear in the sidebar as disabled items with a "Coming" tag. They have no route and no screen.

## How to read the tasks

As in gateway plan 2, each task gives **Interfaces**, **Rules** and **Tests** (test code or case tables where every row is an assertion), all binding, plus implementation notes. The implementer writes the code. Follow TDD: write the tests, see them fail for the expected reason, implement, see them pass.

## Global Constraints

- The console is a static app: no server rendering, no Node process at runtime.
- The app makes requests only to its own origin, and only to paths under `/api`. No third-party requests of any kind: no CDN, no web fonts from another host, no analytics, no error reporting service.
- Dependency versions are the latest stable at implementation time, pinned exactly (no `^` or `~`) in `ui/package.json`; `ui/pnpm-lock.yaml` is committed. The plan names packages, not version numbers; record the versions chosen in the task report.
- TypeScript `strict` is on, with `noUncheckedIndexedAccess` and `exactOptionalPropertyTypes`. No `any`, no `@ts-ignore`, no non-null `!` on API data. `@ts-expect-error` needs a comment saying why.
- API types come only from the file generated from `openapi/admin.json`. No hand-written copy of a request or response shape.
- The session cookie is `HttpOnly`; the app never reads or writes it. The CSRF token lives in memory only: never in `localStorage`, `sessionStorage`, a cookie, the URL, or a log.
- Secrets shown once (new virtual key, new access token, invite link) live in component state only, are never put in the query cache, the router state, the URL or browser storage, and are gone when the dialog closes.
- Passwords and provider API keys are cleared from form state after a successful submit and when the form unmounts.
- No `dangerouslySetInnerHTML`. No inline `style` attributes and no runtime-injected `<style>` (the Content Security Policy forbids them); styling is in CSS files.
- The API decides what a user may do. The console hides what the API would refuse, using the role and teams from `/api/auth/me`, and still handles a 403 or 404 for every call.
- Accessibility: every interactive element is a real `button`, `a` or form control, reachable and usable by keyboard, with a visible focus ring and an accessible name. Dialogs trap focus and close on Escape. Form errors are tied to their field. Text contrast is at least 4.5:1. Status is never conveyed by colour alone.
- Works at 1280 px wide and up. Below that it must remain usable (no overlapping or cut-off controls) down to 1024 px; phone layouts are out of scope.
- Light theme only in this plan.
- The gateway's existing behavior does not change: all 401 Rust tests pass unchanged; `/api`, `/v1` and `/health` answer as before.
- Commit with `git -c user.name=techgopal -c user.email=techgopal2@gmail.com commit`; messages end with the two trailer lines in use on this branch (`Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and the `Claude-Session:` line).

## Visual language (from the prototype)

| Token | Value |
|---|---|
| Ground | `#f6f5f1` |
| Surface | `#ffffff` |
| Ink | `#1a1917` |
| Muted text | `#5f5d57` |
| Border | `#e2e0d8`; control border `#cfcdc6` |
| Sidebar | ground `#1a1917`, text `#cfcdc6`, active item `#34322e` with white text |
| Accent (primary buttons, links) | `#9a3412`; hover `#7c2d12` |
| Data blue (bars, meters) | `#2f5d8a` |
| Status pills | ok `#e3f1e7` / `#14532d`; warn `#fdf0d5` / `#78350f`; error `#fde4e1` / `#7f1d1d`; neutral `#eceae4` / `#3d3b37` |
| Typeface | Instrument Sans for text, JetBrains Mono for ids, keys and numbers, both self-hosted from `@fontsource` packages |
| Radius | 8 px controls, 12 px cards |
| Control height | 44 px |

These are defined once as CSS custom properties in `ui/src/styles/tokens.css`. Components use the properties, never the literal values.

## Review Focus

1. The session expires, or the user is disabled, while the console is open: the next API call gets 401, and the console must go to sign-in, clear every cached response and any secret on screen, and return the user to where they were after they sign in again. Pinned in Task 4.
2. A deep link such as `/keys` or `/accept-invite?token=…` is opened directly or the page is reloaded: the gateway must serve the app for any non-API path, and the app must route to the right screen. Pinned in Tasks 2 and 4.
3. The API refuses a write with 403, 409 or 422 (for example `last_admin`, `team_exists`, a field error): the form shows the API's message next to the right field or as a form error, keeps what the user typed, and does not close. Pinned in Task 5 and each page task.
4. A secret shown once is dismissed by accident, or the user navigates away: it must be gone and not recoverable from the back button, the cache or a reload; the dialog must make the user confirm before it closes. Pinned in Task 5.
5. A member or team lead opens a page or action meant for admins by typing its URL: they see a "not available" screen, not a broken page or an error toast storm. Pinned in Task 4 and Task 10.

## File Structure

```
ui/
  package.json  pnpm-lock.yaml  tsconfig.json  vite.config.ts  eslint.config.js
  index.html
  playwright.config.ts
  src/
    main.tsx                     entry: providers, router
    router.tsx                   route tree and guards
    styles/tokens.css  base.css  components.css
    api/schema.d.ts              generated from openapi/admin.json (committed)
    api/client.ts                fetch wrapper, CSRF, error type
    api/errors.ts                ApiError, field errors
    api/queries.ts               query keys and hooks per resource
    auth/session.tsx             current user, CSRF token, sign-out
    auth/guards.ts               who may see what
    components/                  Shell, Sidebar, PageHeader, DataTable, Field,
                                 Button, Dialog, ConfirmDialog, SecretDialog,
                                 Pill, Toast, EmptyState, ErrorState, Spinner
    pages/                       SignIn, Setup, AcceptInvite, Overview,
                                 Users, UserDetail, Teams, TeamDetail,
                                 Keys, Providers, Account, Audit, NotFound,
                                 NotAvailable
    test/                        MSW handlers, render helpers, fixtures
  e2e/                           Playwright specs and the gateway launcher
crates/gateway/
  build.rs                       copies ui/dist or writes a placeholder
  src/web.rs                     serves the embedded console
```

---

### Task 1: Scaffold, tokens and shell

**Files:**
- Create: everything under `ui/` listed above for the entry, router skeleton, styles, `components/Shell.tsx`, `Sidebar.tsx`, `PageHeader.tsx`, `Button.tsx`, `Pill.tsx`, `Spinner.tsx`, `pages/NotFound.tsx`, `test/render.tsx`
- Modify: `.gitignore` (add `ui/node_modules`, `ui/dist`, `ui/test-results`, `ui/playwright-report`), `.github/workflows/ci.yml`, `.dockerignore`

**Interfaces (produces):**
- `pnpm` scripts in `ui/package.json`: `dev`, `build`, `typecheck`, `lint`, `test`, `test:e2e`, `gen:api`, `check:api`
- `ui/dist/` as the build output, with hashed asset names under `ui/dist/assets/`
- `<Shell>` with the sidebar and a content outlet; `<Sidebar items={…} />`; `<PageHeader title subtitle actions />`; `<Button variant="primary" | "ghost" | "danger">`; `<Pill tone="ok" | "warn" | "error" | "neutral">`
- `renderWithApp(ui, { route?, user? })` test helper

**Rules:**
1. Packages: `react`, `react-dom`, `@tanstack/react-router`, `@tanstack/react-query`, `@tanstack/react-table`, `@tanstack/react-form`, `@fontsource/instrument-sans`, `@fontsource/jetbrains-mono`; dev: `typescript`, `vite`, `@vitejs/plugin-react`, `vitest`, `jsdom`, `@testing-library/react`, `@testing-library/user-event`, `@testing-library/jest-dom`, `msw`, `eslint` with `typescript-eslint`, `eslint-plugin-react-hooks` and `eslint-plugin-jsx-a11y`, `openapi-typescript`, `@playwright/test`. Nothing else without a stated reason in the report. No CSS framework, no component library, no icon package (inline SVG for the few icons).
2. The router is code-based (one `router.tsx`), so no code generation step is needed for routes.
3. Sidebar sections and items, in this order. Items marked coming are rendered as non-interactive text with a "Coming" tag and `aria-disabled="true"`.
   - Observe: Overview (`/`), Logs (coming), Playground (coming)
   - Configure: Providers (`/providers`), Models (coming), Routing (coming), Virtual keys (`/keys`)
   - Govern: Users (`/users`), Teams (`/teams`), Budgets and limits (coming), Guardrails (coming), MCP tools (coming)
   - Bottom: Audit log (`/audit`, admins only), Account (`/account`), and the signed-in user's name, role and a Sign out button
4. The active item is marked with `aria-current="page"`.
5. `vite.config.ts`: `base: "/"`, build output `dist`, asset file names hashed, no source maps in the production build, dev server proxy of `/api`, `/v1` and `/health` to `http://127.0.0.1:3900`.
6. Fonts are imported from the `@fontsource` packages so Vite emits them as local assets. Only the weights used (400, 500, 600, 700 for text; 400, 500 for mono) are imported.
7. ESLint fails on any warning. `jsx-a11y` recommended rules are on.
8. CI gains a job `console`: `pnpm install --frozen-lockfile`, `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm build`, using Node 22 and the pnpm version recorded in `package.json` `packageManager`.

**Tests** (Vitest, Testing Library):

| Test | Assertion |
|---|---|
| `sidebar lists sections in order` | the three section headings and every item text appear in the order of rule 3 |
| `coming items are not links` | "Logs" has no `href`, is `aria-disabled`, and carries the text "Coming" |
| `active item is marked` | at route `/keys`, the "Virtual keys" link has `aria-current="page"` and no other item has it |
| `audit item is for admins` | with a member user the sidebar has no "Audit log"; with an admin it has |
| `unknown route shows not found` | route `/nope` renders the NotFound page with a link back to Overview |
| `tokens are the only colours` | a test reads every `.css` file under `src/styles` except `tokens.css` and every `.tsx` file, and fails if it finds a hex colour literal |
| `no inline styles` | the same scan fails on `style=` in any `.tsx` file |
| `build output is self-contained` | after `pnpm build`, no file in `dist` contains `http://` or `https://` pointing at a host other than in a licence comment or the SVG/XML namespace URIs `www.w3.org` |

- [ ] Step 1: Create the project and write the tests. Run `pnpm test`. Expected: failures (components missing).
- [ ] Step 2: Implement.
- [ ] Step 3: `pnpm lint && pnpm typecheck && pnpm test && pnpm build`. Expected: all pass; report the size of `dist` and of the largest JavaScript file, gzipped.
- [ ] Step 4: Commit `feat(console): scaffold, design tokens and shell`.

---

### Task 2: The gateway serves the console

**Files:**
- Create: `crates/gateway/build.rs`, `crates/gateway/src/web.rs`, `crates/gateway/tests/web.rs`
- Modify: root and gateway `Cargo.toml` (add `rust-embed` with the `interpolate-folder-path` feature, and `mime_guess`), `crates/gateway/src/lib.rs`, `src/app.rs`, `Dockerfile`, `.dockerignore`, `README.md`

**Interfaces (produces):**
- `web::router() -> axum::Router<Arc<AppState>>` merged into the app router
- `web::CONSOLE_BUILT: bool`, true when a real console build was embedded

**Rules:**
1. `build.rs` looks for `ui/dist/index.html` relative to the workspace root. If it exists it copies `ui/dist` into `$OUT_DIR/console`. If not, it writes a single placeholder `index.html` there that says the console was not built and names the command to build it. It emits `cargo:rerun-if-changed` for `ui/dist` and sets `cargo:rustc-cfg` so `CONSOLE_BUILT` is known at compile time. It never runs `pnpm` or `npm`. So `cargo build` works on a machine without Node.
2. `web.rs` embeds `$OUT_DIR/console` and serves:
   - `GET /` and `HEAD /`: `index.html`.
   - `GET /assets/<file>`: the embedded file, or 404 if missing. Never `index.html` for a missing asset.
   - Any other `GET` or `HEAD` path that does not start with `/api/`, `/v1/` or equal `/api`, `/v1`, `/health`: `index.html`, so deep links work. This is the fallback of the app router.
   - Other methods on those paths: 405.
3. `/api/*`, `/v1/*` and `/health` keep their handlers and their own 404 and 405 bodies. An unknown `/api/...` path still answers the `/api` JSON 404, not `index.html`. An unknown `/v1/...` path answers 404 in the OpenAI error shape.
4. Headers on `index.html`: `Cache-Control: no-cache`, and the security headers of rule 6. Headers on `/assets/*`: `Cache-Control: public, max-age=31536000, immutable`, `X-Content-Type-Options: nosniff`, and the right `Content-Type` from the file extension. `ETag` is set from the embedded file's hash and `If-None-Match` answers 304.
5. Path handling: the requested path is never used to read from disk. Paths containing `..`, a backslash, a NUL or a percent-encoded form of those answer 404.
6. Security headers on every HTML response:
   - `Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'`
   - `X-Content-Type-Options: nosniff`
   - `Referrer-Policy: no-referrer`
   - `X-Frame-Options: DENY`
   - `Cross-Origin-Opener-Policy: same-origin`
   - `Permissions-Policy: camera=(), microphone=(), geolocation=()`
   `/api` JSON responses also gain `X-Content-Type-Options: nosniff` and `Cache-Control: no-store`.
7. Dockerfile: a Node stage (`node:22-slim`, pnpm through corepack) builds `ui/dist`; the Rust stage copies `ui/dist` in before `cargo build`. The final image is unchanged otherwise. `.dockerignore` stops excluding `ui` but excludes `ui/node_modules` and `ui/dist`.
8. README: how to build the console and the binary together (`pnpm --dir ui install --frozen-lockfile && pnpm --dir ui build && cargo build --release -p ultrafast-gateway`), and how to develop (`pnpm --dir ui dev` with the gateway on port 3900 started with `--insecure-cookies`).

**Tests** (`crates/gateway/tests/web.rs`, using the existing harness):

| Test | Assertion |
|---|---|
| `root serves html` | `GET /` is 200 with `content-type` starting `text/html` |
| `deep links serve the app` | `GET /keys`, `/users/12`, `/accept-invite?token=x` each give the same body as `/` |
| `api and v1 are not shadowed` | `GET /api/nope` is 404 with `error.code == "not_found"` as JSON; `GET /v1/nope` is 404 JSON in the OpenAI shape; `GET /health` is 200 JSON; `POST /v1/chat/completions` without a key is still 401 |
| `missing asset is 404 not html` | `GET /assets/nope.js` is 404 and its body is not the index page |
| `html has the security headers` | every header of rule 6 is present with the exact value |
| `html is not cached, assets are` | header values of rule 4 |
| `post to a page path is 405` | `POST /keys` |
| `traversal is refused` | `/assets/../Cargo.toml`, `/assets/..%2f..%2fCargo.toml`, `/assets/%2e%2e/x`, `/assets/a\b` are 404 |
| `api responses are not cached or sniffed` | `GET /api/setup` has `cache-control: no-store` and `x-content-type-options: nosniff` |
| `etag gives 304` | second request with `If-None-Match` set to the first response's `ETag` is 304 with an empty body |
| `placeholder when not built` | when `CONSOLE_BUILT` is false, `/` contains the text "console was not built"; when true the test asserts the page contains `<div id="root">` |

- [ ] Step 1: Write the tests. Run `cargo test -p ultrafast-gateway --test web`. Expected: compile errors (`web` not found).
- [ ] Step 2: Implement.
- [ ] Step 3: Run `cargo test --all` twice: once with `ui/dist` absent and once after `pnpm --dir ui build`. Expected: all pass both times. Report the release binary size with and without the console.
- [ ] Step 4: Build the Docker image, run it, and fetch `/` and one asset; record status and headers. Remove the image afterwards.
- [ ] Step 5: Commit `feat(gateway): serve the embedded console`.

---

### Task 3: API client

**Files:**
- Create: `ui/src/api/schema.d.ts` (generated), `api/client.ts`, `api/errors.ts`, `api/queries.ts`, `ui/src/test/handlers.ts`, `test/fixtures.ts`
- Modify: `ui/package.json`, `.github/workflows/ci.yml`

**Interfaces (produces):**

```ts
// api/errors.ts
export class ApiError extends Error {
  readonly status: number
  readonly code: string            // "forbidden", "last_admin", "validation_failed", …
  readonly fields: Readonly<Record<string, string>>   // empty when none
}
export class NetworkError extends Error {}          // the request did not complete

// api/client.ts
export function setCsrfToken(token: string | null): void
export function onUnauthenticated(handler: () => void): () => void   // returns unsubscribe
export const api: {
  get<P extends GetPath>(path: P, opts?): Promise<ResponseOf<P, "get">>
  post / patch / put / delete  // typed the same way from the generated paths
}
```

**Rules:**
1. `pnpm gen:api` runs `openapi-typescript ../openapi/admin.json -o src/api/schema.d.ts`. `pnpm check:api` regenerates to a temporary file and fails if it differs from the committed one. CI runs `check:api` in the `console` job.
2. Every request is same-origin with `credentials: "same-origin"`, `Accept: application/json`, and `Content-Type: application/json` when it has a body.
3. For methods other than GET and HEAD the client adds `x-csrf-token` when a token is set. It never adds it to GET.
4. A response with status 204, or with no body, resolves to `undefined`.
5. A non-2xx response with the `/api` error shape becomes an `ApiError` with its `status`, `code`, `message` and `fields`. A non-2xx response with any other body (HTML from a proxy, empty, invalid JSON) becomes an `ApiError` with code `unexpected_response` and a message that names the status only; the body is not shown.
6. A failed `fetch` (offline, connection refused) becomes a `NetworkError` with the message "Could not reach the gateway."
7. A 401 on any call except `POST /api/auth/login`, `POST /api/auth/accept-invite`, `POST /api/auth/password` and `GET /api/auth/me` calls every `onUnauthenticated` handler once, then rejects with the `ApiError`. A 401 from `POST /api/auth/password` means the current password was wrong and must not sign the user out.
8. Path parameters are filled by the client from a `params` object and are encoded with `encodeURIComponent`. A missing parameter is a thrown error, not a request to a broken URL.
9. `queries.ts` defines one query key factory and the hooks used by pages: lists, details and mutations per resource. A mutation invalidates the lists and details it affects. Retries: queries retry network errors up to 2 times and never retry an `ApiError`; mutations never retry.
10. Nothing in the client logs request or response bodies.

**Tests** (Vitest with MSW):

| Test | Assertion |
|---|---|
| `get resolves typed data` | `api.get("/api/teams")` returns the fixture |
| `csrf header on writes only` | a POST carries `x-csrf-token`; a GET does not; with no token set a POST carries none |
| `204 resolves undefined` | |
| `api error is parsed` | 409 with `{error:{code:"team_exists",message:"…"}}` rejects with `ApiError` having those values and empty `fields` |
| `field errors are kept` | 422 with `fields` gives `error.fields.email` |
| `html error body is not shown` | a 502 with an HTML body rejects with code `unexpected_response` and a message without `<` |
| `network failure` | rejects with `NetworkError` |
| `401 signs out once` | two concurrent calls that both get 401 call the handler once |
| `401 on sign-in, invite, password and me does not sign out` | handler not called for those four |
| `path params are encoded` | `params: {id: "a/b"}` requests `/api/teams/a%2Fb` |
| `missing path param throws` | |
| `mutations invalidate` | after a successful "create team", the teams list query refetches |
| `api errors are not retried` | a query that gets 500 is called once; one that gets a network error is called 3 times |
| `generated types are current` | `pnpm check:api` exits 0 |

- [ ] Step 1: Generate the schema, write the tests. Run. Expected: failures.
- [ ] Step 2: Implement.
- [ ] Step 3: `pnpm lint && pnpm typecheck && pnpm test`. Expected: pass.
- [ ] Step 4: Commit `feat(console): typed API client`.

---

### Task 4: Session, guards and the auth screens

**Files:**
- Create: `ui/src/auth/session.tsx`, `auth/guards.ts`, `pages/SignIn.tsx`, `pages/Setup.tsx`, `pages/AcceptInvite.tsx`, `pages/NotAvailable.tsx`
- Modify: `ui/src/router.tsx`, `main.tsx`, `components/Sidebar.tsx`

**Interfaces (produces):**

```ts
export type Me = { user: UserView; teams: { team_id: number; name: string; role: TeamRole }[] }
export function useSession(): { status: "loading" } | { status: "signedOut" } | { status: "signedIn"; me: Me }
export function useSignOut(): () => Promise<void>

// guards.ts: pure functions of Me
export function isAdmin(me: Me): boolean
export function leads(me: Me, teamId: number): boolean
export function ledTeamIds(me: Me): number[]
export function can(me: Me, action: ConsoleAction): boolean   // mirrors the API's policy for showing controls
```

**Rules:**
1. On load the app calls `GET /api/setup` and `GET /api/auth/me`. If setup is needed every route redirects to `/setup`. If `me` is 401 the user is signed out.
2. Public routes: `/sign-in`, `/setup`, `/accept-invite`. Every other route needs a signed-in user; a signed-out visitor is sent to `/sign-in?next=<path>`.
3. `next` is accepted only if it starts with a single `/` and not `//` or `/\`; anything else is ignored and the user lands on `/`. After sign-in the user goes to `next`.
4. A signed-in user who opens `/sign-in` or `/setup` is sent to `/`. `/accept-invite` signs the current user out first, after asking.
5. Sign-in form: email, password, a submit button. On 401 it shows "Email or password is incorrect." On 429 it shows "Too many attempts. Try again in a few minutes." On a network error it shows the network message. The password field is cleared after any failed attempt; the email is kept. The button is disabled while the request runs.
6. After sign-in the CSRF token from the response is set in the client; after a reload it comes from `GET /api/auth/me`.
7. Setup form: name, email, password, confirm password. Field errors from a 422 are shown on their fields. Passwords that differ are caught before any request. On 409 `already_set_up` it shows that setup is complete and links to sign-in. On success it goes to sign-in with a notice.
8. Accept invite: reads `token` from the query string once, then removes it from the URL with `history.replaceState` so it is not left in the address bar or history. Fields: password, confirm. 404 shows "This invite link is not valid or has expired. Ask an admin for a new one." On success it goes to sign-in with a notice.
9. Password fields show the policy ("12 characters or more") and use `autocomplete="new-password"`; sign-in uses `current-password` and `username`.
10. On an unauthenticated event: clear the CSRF token, clear the whole query cache, close any dialog, and go to `/sign-in?next=<current path>` with the notice "Your session ended. Sign in again."
11. Sign out calls `POST /api/auth/logout`, then does the same clean-up and goes to `/sign-in`. If the call fails the clean-up still happens.
12. Routes for admins only (`/audit`) and any page whose data call returns 403 render `NotAvailable` ("This page is not available to your account."), not an error.
13. `can` mirrors the API policy for the actions the console shows: invite user, edit user role or status, delete user, create team, rename team, delete team, add member, make lead, remove member, create key for self, create key for a member of a led team, revoke key, manage providers, view audit.

**Tests:**

| Test | Assertion |
|---|---|
| `setup needed redirects everything` | with `needs_setup: true`, `/`, `/keys` and `/sign-in` all render Setup |
| `signed out visitor goes to sign-in with next` | `/keys` ends at `/sign-in?next=%2Fkeys` |
| `unsafe next is ignored` | `next=//evil.example`, `next=https://evil.example`, `next=/\evil`, `next=javascript:alert(1)` all land on `/` after sign-in |
| `sign-in success` | lands on `next`; the sidebar shows the user's name and role; a later POST carries the CSRF token |
| `sign-in failures` | 401, 429 and network error show the three messages of rule 5; password cleared, email kept |
| `submit is disabled while signing in` | |
| `reload keeps the session` | a fresh app with `me` returning 200 renders the page without visiting sign-in, and writes carry the token from `me` |
| `setup validates before sending` | differing passwords send no request |
| `setup shows field errors` | 422 with `fields.email` shows the text under the email field, linked by `aria-describedby` |
| `setup already done` | 409 shows the message and a link to sign-in |
| `invite token leaves the URL` | after render, `location.search` has no `token`; submit still sends it |
| `invalid invite` | 404 shows the message of rule 8 |
| `session ending mid-use` | on `/keys` with data shown, the next call returns 401: the app is at `/sign-in?next=%2Fkeys`, shows the notice, and the query cache is empty |
| `session ending closes a secret dialog` | with a SecretDialog open, a 401 elsewhere removes the secret from the document |
| `sign out cleans up even if the call fails` | |
| `audit is not available to members` | a member at `/audit` sees NotAvailable and no request to `/api/audit` is made |
| `403 from a page renders not available` | |
| `guards` | table-driven over an admin, a lead of team 1 who is a member of team 2, a plain member, and a user with no team, for every action in rule 13 |

- [ ] Step 1: Write the tests. Run. Expected: failures.
- [ ] Step 2: Implement.
- [ ] Step 3: `pnpm lint && pnpm typecheck && pnpm test`. Expected: pass.
- [ ] Step 4: Commit `feat(console): session, guards and auth screens`.

---

### Task 5: Shared components

**Files:**
- Create: `ui/src/components/DataTable.tsx`, `Field.tsx`, `Dialog.tsx`, `ConfirmDialog.tsx`, `SecretDialog.tsx`, `Toast.tsx`, `EmptyState.tsx`, `ErrorState.tsx`, `FormError.tsx`, `ui/src/components/form.ts`

**Interfaces (produces):**

```tsx
<DataTable columns rows getRowId empty={<EmptyState …/>} caption="…" />   // TanStack Table, client-side sort
<Field label name error? hint? required?>{control}</Field>
<Dialog title open onClose>{…}</Dialog>
<ConfirmDialog title body confirmLabel tone="danger" | "primary" onConfirm />   // onConfirm may reject with ApiError
<SecretDialog title secret description />      // shows a secret once
<ErrorState error onRetry? />                   // for failed queries
useToast(): (message: string, tone?: "ok" | "error") => void
applyApiError(form, error): void                // puts ApiError.fields on fields, the rest on the form
```

**Rules:**
1. `Dialog` uses the native `<dialog>` element opened with `showModal()`: focus moves into it, Tab stays inside, Escape closes, and focus returns to the control that opened it. The title is its accessible name.
2. `ConfirmDialog` disables its buttons while `onConfirm` runs. If `onConfirm` rejects with an `ApiError` the dialog stays open and shows the message; it closes only on success or cancel.
3. `SecretDialog`:
   - shows the secret in a read-only mono field with a Copy button and the description (for example "Copy this key now. It is not shown again.");
   - Copy uses `navigator.clipboard.writeText` and confirms with "Copied"; if the clipboard is not available it selects the text and says "Press Ctrl+C to copy";
   - closing (button, Escape, backdrop) first asks "Have you copied it? It cannot be shown again." with Keep open as the default;
   - the secret is held in the state of the component that opened the dialog and is set to `null` on close; it is never passed to a toast, a log, the query cache or the router.
4. `DataTable` renders a real `<table>` with a `<caption>` (visually hidden is fine), `<th scope="col">`, and sortable headers as buttons with `aria-sort`. Sorting is client-side. An empty list renders the `empty` element, not an empty table. A loading list renders 5 skeleton rows with `aria-busy="true"`.
5. `Field` ties its label, hint and error to the control with `htmlFor`, `aria-describedby` and `aria-invalid`.
6. `applyApiError`: each key of `error.fields` that matches a form field is set as that field's error; keys that match no field, and the error's message when there are no field errors, are shown by `FormError` at the top of the form with `role="alert"`. The form keeps every value the user typed.
7. `ErrorState` shows the error's message and a Retry button when `onRetry` is given. For a `NetworkError` it shows the network message. It never shows a stack trace or a raw response.
8. Toasts are announced with `role="status"`, disappear after 5 seconds, and can be dismissed. Errors from mutations are shown in the form or dialog that caused them, not as toasts. Toasts are for successes.
9. Dates from the API (`YYYY-MM-DD HH:MM:SS`, UTC) are shown in the browser's locale and time zone, with the exact UTC value in a `title` attribute; a helper `formatTimestamp` does this and returns "Never" for null.

**Tests:**

| Test | Assertion |
|---|---|
| `dialog focus` | opening moves focus inside; Tab from the last control goes to the first; Escape closes; focus returns to the opener |
| `confirm stays open on api error` | `onConfirm` rejecting with `last_admin` keeps the dialog and shows "At least one active admin is required." |
| `confirm buttons disabled while running` | |
| `secret is shown and copied` | the secret text is present; Copy calls the clipboard with it and shows "Copied" |
| `secret copy fallback` | with no clipboard API the field is selected and the fallback text shows |
| `secret close asks first` | Escape shows the question; Keep open keeps the secret; Close removes the secret from the document |
| `secret is gone after close` | after closing, the secret string is nowhere in `document.body.innerHTML`, and the query cache has no entry containing it |
| `table sorts` | clicking a header sorts ascending, again descending, with `aria-sort` set |
| `table empty and loading` | the two states of rule 4 |
| `field wiring` | label click focuses the control; error is announced through `aria-describedby`; `aria-invalid="true"` when there is an error |
| `api field errors land on fields` | 422 with `fields: {name: "must not be empty", other: "x"}` puts the first on the name field and shows `other` in the form error |
| `form keeps values after an error` | |
| `error state` | an `ApiError` shows its message and Retry calls the handler; a `NetworkError` shows the network message |
| `timestamps` | `"2026-09-28 14:10:00"` renders a localized string with `title="2026-09-28 14:10:00 UTC"`; null renders "Never" |

- [ ] Step 1: Write the tests. Run. Expected: failures.
- [ ] Step 2: Implement.
- [ ] Step 3: `pnpm lint && pnpm typecheck && pnpm test`. Expected: pass.
- [ ] Step 4: Commit `feat(console): shared components`.

---

### Task 6: Users

**Files:**
- Create: `ui/src/pages/Users.tsx`, `pages/UserDetail.tsx`
- Modify: `ui/src/router.tsx`, `api/queries.ts`

**Routes:** `/users`, `/users/$id`

**Rules:**
1. `/users` lists what `GET /api/users` returns: name, email, role, status, teams are not in this response so the list does not show them, last active. Status is a pill: `active` ok, `invited` warn, `disabled` neutral.
2. Admins see an "Invite user" button. It opens a dialog: name, email, role (Member or Admin). On success the dialog is replaced by a `SecretDialog` holding the invite link as an absolute URL built from `window.location.origin` and the `invite_link` path the API returned, with the description "Send this link to the user. It works once and expires in 7 days."
3. A row links to `/users/$id`. The detail page shows the user's fields and, for admins, controls: edit name, change role, disable or enable, resend invite (only when `invited`), delete.
4. Every user can edit their own name on their own detail page. Non-admins see no role, status or delete controls, even on their own page.
5. Changing role, disabling, and deleting each go through a `ConfirmDialog` that states the consequence:
   - role: "Their current sessions end and they must sign in again."
   - disable: "They are signed out, their access tokens are revoked, and their virtual keys stop working until they are enabled again."
   - delete of an active user: "Their virtual keys keep working without an owner. Revoke the keys first if they should stop."
   - delete of a user who is not active: "Their virtual keys are revoked."
6. API refusals are shown in the dialog that caused them, with the API's message: `last_admin`, `cannot_delete_self`, `no_password`, `not_invited`, `user_exists`.
7. A user the caller may not see (404 from the API) renders the NotFound page.
8. The signed-in user's own row is marked "You".

**Tests:**

| Test | Assertion |
|---|---|
| `admin sees the list and the invite button` | |
| `member sees only what the API returns and no invite button` | |
| `invite shows the link once` | the dialog shows an absolute URL starting with the origin and containing `/accept-invite?token=uf-inv-`; after closing, the link is nowhere in the document and the users list has refetched |
| `invite errors` | 409 `user_exists` shows on the email field; 422 field errors show on their fields; the dialog stays open with values kept |
| `status pills` | the three statuses have their text and tone |
| `own row is marked` | |
| `detail controls by role` | admin sees all controls; a member on their own page sees only name edit; a lead on a team member's page sees no controls |
| `resend invite only when invited` | |
| `role change confirms and reports` | the consequence text of rule 5 is shown; on success the detail shows the new role and a toast |
| `last admin refusal stays in the dialog` | |
| `delete texts differ by status` | active and disabled users show the two texts of rule 5 |
| `delete returns to the list` | |
| `hidden user is not found` | 404 renders NotFound |

- [ ] Step 1: Write the tests. Run. Expected: failures.
- [ ] Step 2: Implement.
- [ ] Step 3: `pnpm lint && pnpm typecheck && pnpm test`. Expected: pass.
- [ ] Step 4: Commit `feat(console): users`.

---

### Task 7: Teams

**Files:**
- Create: `ui/src/pages/Teams.tsx`, `pages/TeamDetail.tsx`
- Modify: `ui/src/router.tsx`, `api/queries.ts`

**Routes:** `/teams`, `/teams/$id`

**Rules:**
1. `/teams` lists name, member count and created date. Admins see "New team" (a dialog with one field, name).
2. `/teams/$id` shows the team name and its members (name, email, role in the team). Controls depend on `can`:
   - admin: rename, delete, add member, make lead, make member, remove member;
   - lead of this team: rename, add member as member, remove member. No "make lead".
   - member of this team: no controls.
3. Add member: admins choose from a list of users (from `GET /api/users`). A lead is given a field for the user's id, because the API does not let a lead list users outside their teams; the field is labelled "User ID" with the hint "Ask an admin for the user's ID." This limitation is recorded in Known limits.
4. Delete team confirms with "Keys that belong to this team keep working and lose their team."
5. Removing a member confirms with the member's name. A lead removing themselves is told "You will lose access to this team."
6. API refusals shown in place: `team_exists`, `user_disabled`, and 404 for a user id that does not exist ("No user with that ID.").

**Tests:**

| Test | Assertion |
|---|---|
| `list shows counts` | |
| `new team is for admins` | |
| `duplicate name stays in the dialog` | `team_exists` on the name field |
| `detail controls by role` | the three cases of rule 2 |
| `lead cannot make a lead` | no such control; and if the API returns 403 for any reason the message is shown in place |
| `admin picks a user from a list` | the list excludes users already in the team and disabled users |
| `lead adds by id` | sends `PUT /api/teams/$id/members/$userId` with role member; 404 shows "No user with that ID." |
| `remove confirms with the name` | |
| `lead removing themselves is warned` | after success they are sent to `/teams` |
| `delete confirms with the consequence` | |
| `hidden team is not found` | |

- [ ] Step 1: Write the tests. Run. Expected: failures.
- [ ] Step 2: Implement.
- [ ] Step 3: `pnpm lint && pnpm typecheck && pnpm test`. Expected: pass.
- [ ] Step 4: Commit `feat(console): teams`.

---

### Task 8: Virtual keys and providers

**Files:**
- Create: `ui/src/pages/Keys.tsx`, `pages/Providers.tsx`
- Modify: `ui/src/router.tsx`, `api/queries.ts`

**Routes:** `/keys`, `/providers`

**Rules for keys:**
1. The list shows name, key (the masked `display`, in mono), owner, team, expires, status. Status pills: `active` ok, `suspended` warn with the hint "The owner is not active", `expired` neutral, `revoked` neutral. A key with no owner shows "No owner".
2. Filters above the table, all client-side: text search over name, owner and display; team; status. "Show revoked" is off by default.
3. "Create key" opens a dialog: name; team (none, or one of the teams the chosen owner belongs to); owner (admins: any active user; leads: themselves or a member of a team they lead; members: themselves, shown as fixed text); expires (never, 30 days, 90 days, or a date). The expiry is sent as UTC `YYYY-MM-DD HH:MM:SS` at the end of the chosen day.
4. On success the `secret` is shown in a `SecretDialog` with "Copy this key now. It is not shown again.", together with a short example of using it: the base URL of this gateway followed by `/v1` and the header `Authorization: Bearer <key>`. The example shows the placeholder, not the secret.
5. Revoke confirms with "Apps using this key stop working at once. This cannot be undone."
6. Field errors from the API show on their fields, including `team_id` ("owner is not a member of this team").

**Rules for providers:**
7. Everyone sees the list: name, kind, base URL, and whether a credential is set ("Set" or "None"). Only admins see Add, Edit and Delete.
8. Add: name, kind (OpenAI-compatible or Anthropic), base URL, API key. Hints: name is "lowercase letters, digits, - and _"; for OpenAI-compatible the base URL usually ends in `/v1`. Known base URLs are offered as choices that fill the field: OpenAI `https://api.openai.com/v1`, Anthropic `https://api.anthropic.com`, Groq `https://api.groq.com/openai/v1`, Mistral `https://api.mistral.ai/v1`, OpenRouter `https://openrouter.ai/api/v1`, Ollama `http://localhost:11434/v1`.
9. The API key field is `type="password"` with `autocomplete="off"`, has a Show toggle, and is cleared after submit and on close.
10. Edit: base URL, and credential with three choices: keep the current one (sends no `api_key`), replace (sends the new value), remove (sends `api_key: null`). The current credential is never shown, because the API never returns it.
11. After a provider is added the page shows how to call it: model names are `<provider name>/<model>`.
12. Delete confirms with "Calls to models of this provider will fail at once."

**Tests:**

| Test | Assertion |
|---|---|
| `keys list and pills` | the four statuses; "No owner" for an ownerless key; suspended shows its hint |
| `revoked keys are hidden by default` | and appear when "Show revoked" is on |
| `filters` | text, team and status each narrow the list; combined they intersect |
| `member creates a key for themselves` | owner is fixed text; request has no `owner_id` or the member's own |
| `lead can choose a team member` | the owner list has themselves and members of led teams only |
| `team choices follow the owner` | changing owner resets team to none and lists that owner's teams |
| `expiry is sent in UTC` | choosing a date sends `YYYY-MM-DD 23:59:59`; "never" sends no `expires_at` |
| `new key is shown once` | SecretDialog shows the secret; the example shows `<key>`, not the secret; after closing the secret is nowhere in the document or the query cache; the list has refetched |
| `field error on team` | 422 `fields.team_id` shows under the team field |
| `revoke confirms` | consequence text; on success the key's pill is `revoked` |
| `providers list hides admin controls from others` | |
| `known base URLs fill the field` | choosing Groq sets the URL and the kind to OpenAI-compatible |
| `api key field is cleared` | after submit and after cancel the field's value is empty |
| `edit sends the three credential choices` | keep sends no `api_key` key at all; replace sends the string; remove sends `null` |
| `provider errors` | `provider_exists` on name; 422 fields on their fields |
| `delete confirms` | |

- [ ] Step 1: Write the tests. Run. Expected: failures.
- [ ] Step 2: Implement.
- [ ] Step 3: `pnpm lint && pnpm typecheck && pnpm test`. Expected: pass.
- [ ] Step 4: Commit `feat(console): virtual keys and providers`.

---

### Task 9: Overview, account and audit log

**Files:**
- Create: `ui/src/pages/Overview.tsx`, `pages/Account.tsx`, `pages/Audit.tsx`
- Modify: `ui/src/router.tsx`, `api/queries.ts`

**Routes:** `/`, `/account`, `/audit`

**Rules:**
1. Overview shows only facts the API has today, from the list calls the user is allowed to make: number of providers and how many have a credential; number of virtual keys by status; number of users by status and number of teams (for admins and leads, scoped as the API scopes them). Each tile links to its page. There are no charts, no spend and no request counts, and no sample numbers. A note says: "Usage, spend and request logs arrive with a later release."
2. When there is no provider the overview shows a "Get started" panel with three steps, each linking to its page and marked done when true: add a provider, create a virtual key, make a first call. The third shows a `curl` example against this gateway's origin with placeholders for the key and the model.
3. Account has three parts:
   - Profile: name (editable), email, role, teams with the user's role in each.
   - Password: current, new, confirm. On success: "Password changed. Your other sessions and all your access tokens were ended." 401 shows "Current password is incorrect." on the current field and does not sign the user out. 429 shows the too-many-attempts message.
   - Access tokens: list (name, masked token, expires, last used, status), Create (name, expiry) showing the secret once with "Use it as `Authorization: Bearer <token>` with the admin API.", and Revoke with confirmation.
4. Audit (admins): a table of time, actor, action, summary, newest first, 50 per page, with "Load older" using `before=<last id>`. A text filter narrows the loaded rows client-side. Actions are shown as the API returns them (`user.invite`), in mono.
5. The three password fields are cleared after success and after failure.

**Tests:**

| Test | Assertion |
|---|---|
| `overview counts` | tiles show counts computed from the fixtures and link to their pages |
| `overview has no invented numbers` | the page contains no `$` amount and none of the words "spend", "requests" except in the note |
| `get started appears without providers` | and step 1 is marked done once a provider exists |
| `overview for a member` | shows keys and providers only; makes no call that would return 403 |
| `profile name edit` | |
| `password change success` | message of rule 3; fields cleared |
| `wrong current password` | shown on the current field; the user is still signed in; fields cleared |
| `password mismatch sends nothing` | |
| `token is shown once` | same checks as for keys |
| `token revoke` | |
| `audit lists newest first` | |
| `load older uses before` | the second request has `before` equal to the last id of the first page; the button disappears when a page has fewer than 50 rows |
| `audit filter` | narrows loaded rows |

- [ ] Step 1: Write the tests. Run. Expected: failures.
- [ ] Step 2: Implement.
- [ ] Step 3: `pnpm lint && pnpm typecheck && pnpm test`. Expected: pass.
- [ ] Step 4: Commit `feat(console): overview, account and audit log`.

---

### Task 10: End-to-end tests and release build

**Files:**
- Create: `ui/playwright.config.ts`, `ui/e2e/gateway.ts` (starts the real binary), `ui/e2e/*.spec.ts`
- Modify: `.github/workflows/ci.yml`, `README.md`

**Rules:**
1. The end-to-end tests run against the real `ultrafast` binary with the console embedded, on a free port, with a fresh temporary data directory, `--insecure-cookies`, and an admin from `UF_ADMIN_EMAIL` and `UF_ADMIN_PASSWORD`. No API mocking. The launcher builds nothing: CI builds the console and the binary first.
2. A mock provider (a small local HTTP server in the test process) stands in for an upstream model API, so the "first call" flow can be tested without a real provider.
3. Browsers: Chromium only in CI.
4. The tests fail on any browser console error, any Content Security Policy violation report, and any request to another origin.
5. Flows:

| Spec | Flow |
|---|---|
| `setup.spec` | a gateway with no users shows Setup at `/`; creating the admin leads to sign-in; signing in lands on Overview |
| `signin.spec` | wrong password shows the message; right password signs in; reload stays signed in; sign out returns to sign-in and `/keys` then redirects to sign-in |
| `deep-link.spec` | opening `/keys` signed out goes to sign-in and back to `/keys` after signing in; reloading `/teams/1` shows the team |
| `invite.spec` | admin invites a user, copies the link from the dialog, signs out; the link opens Accept invite, the token is not in the address bar afterwards; the new user sets a password and signs in |
| `keys.spec` | admin adds a provider pointing at the mock, creates a key, copies it, and a `fetch` from the test to `/v1/chat/completions` with that key returns the mock's answer; after Revoke the same call returns 401 |
| `roles.spec` | a member sees no Invite, no New team, no provider controls, no Audit item; typing `/audit` shows "not available"; a lead sees controls for their team only |
| `last-admin.spec` | the only admin trying to disable themselves sees the API's message in the dialog and stays an admin |
| `session-end.spec` | with `/keys` open, the session row is removed (by signing out in a second context); the next action lands on sign-in with the notice, and signing in returns to `/keys` |
| `a11y.spec` | on sign-in, overview, users, keys and the create-key dialog: every control is reachable with Tab in a sensible order, the dialog traps focus, and an automated check (`@axe-core/playwright`, added as a dev dependency) reports no violations |
| `headers.spec` | the document response has the Content Security Policy and the other headers of Task 2 rule 6; assets are served with the immutable cache header |

6. CI: the `console` job builds the console; a new job `e2e` builds the console, builds the binary (so it embeds the console), installs Chromium, and runs `pnpm test:e2e`. It uploads the Playwright report when it fails.
7. README gains a "Console" section: what is in it, what is not yet, how to build, how to develop, and the Known limits below.

- [ ] Step 1: Write the launcher and the specs. Run `pnpm test:e2e`. Expected: failures only where a flow finds a real defect; record each defect found with the spec that found it.
- [ ] Step 2: Fix defects in the console code. A defect in the gateway API is reported, not fixed here.
- [ ] Step 3: Run the whole suite three times in a row. Expected: all pass each time (no flaky test).
- [ ] Step 4: Run `pnpm lint && pnpm typecheck && pnpm test && pnpm build`, then `cargo test --all` and `cargo build --release -p ultrafast-gateway`. Report the release binary size, and the gzipped size of the console's JavaScript and CSS.
- [ ] Step 5: Commit `test(console): end-to-end flows` and any fix commits.

---

## Known limits of this plan

- A team lead adds a member by user ID, because the API does not let a lead find users outside their teams. The project owner has an open decision on adding members by email; when the API changes, the console's add-member dialog changes with it.
- The users list does not show each user's teams, because the API's list response does not include them.
- No usage, spend, request logs, models, routes or budgets: their backends do not exist yet.
- Light theme only; desktop widths only.
- Sign-in limiting counts the reverse proxy's address when the gateway is behind one.

## Spec coverage

| Spec section 13 requirement | Covered here | Deferred to |
|---|---|---|
| Static single-page app compiled into the binary; no Node runtime | Tasks 1, 2 | |
| TanStack Router, Query, Table, Form; built with Vite | Tasks 1, 3, 5 | |
| Pages: Overview, Providers, Virtual keys, Users and teams, Settings (sign-in, backup, audit log) | Tasks 6 to 9 (overview without usage; audit log; account) | retention and backup settings: gateway plan 5 |
| Pages: Logs, Playground, Models, Routing, Budgets and limits | shown as coming | console plans 2 to 4 |
| What a user sees depends on role; the API enforces | Tasks 4, 6 to 9, 10 | |
| Types generated from the OpenAPI spec | Task 3 | |
