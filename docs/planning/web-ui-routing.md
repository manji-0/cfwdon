# Web UI Routing Modernization

<!-- constrained-by ../architecture/cfwdon-architecture.md#operational-plan -->
<!-- constrained-by ../getting-started/development.md -->

Investigation of how `web-ui` uses React Router today, and whether a more modern routing model should replace it. This is a planning note, not an implementation commitment.

## Summary

`web-ui` already depends on **React Router 8.3**, but it uses that library in **Declarative Mode**: `BrowserRouter` plus a flat `<Routes>` table. That is the oldest React Router 8 surface. The app is a Vite SPA under `/app`, served by the Rust Worker. Mastodon API calls, Auth0 login/logout, streaming, and view caching all live outside the router.

The real gap is not “an old package.” It is **three competing sources of truth** for paths:

1. The JSX route table in `web-ui/src/ui/App.tsx`
2. The typed `AppRoute` ADT in `web-ui/src/domain/navigation/route.ts`
3. String literals in `Link`, `navigate(...)`, `GoChord`, and mention HTML

**Preferred destination:** TanStack Router in SPA library mode, with a **code-based** route tree. Keep the Rust Worker as the HTTP server. Do not adopt React Router Framework Mode, TanStack Start, Next.js, or Remix.

**No-regret first step, independent of library choice:** finish `AppRoute` so every in-app path has a typed constructor, then stop writing raw path strings.

## Current Baseline

<!-- derived-from ../architecture/cfwdon-architecture.md#operational-plan -->

Vite builds the SPA with `base: "/app/"`. The Worker serves hashed assets from disk and falls every other `/app/*` path back to `index.html`. Login and logout stay on the Worker (`/app/login`, `/app/logout`). Client history is therefore a SPA concern; deep links work because the Worker already rewrites them.

React Router usage today:

| Surface | Where | Role |
| --- | --- | --- |
| `BrowserRouter` `basename="/app"` | `App.tsx` | History + `/app` prefix |
| `<Routes>` / `<Route>` / `<Navigate>` | `App.tsx` | 24 path entries, including redirects |
| `Link` / `NavLink` | pages, cards, nav | Client navigation and active styles |
| `useNavigate` | keyboard, status actions, threads, messages | Programmatic moves |
| `useParams` | thread, profile, tag, collections | Untyped `string \| undefined` params |
| `useLocation` | nav hubs, public timeline | Pathname-based UI state |
| `useSearchParams` | search | `q` and `type` query state |
| `MemoryRouter` | Vitest helpers | Page and keyboard tests |
| `React.lazy` | `lazy-pages.ts` | Manual route code splitting |

Session loading wraps the route tree. Anonymous users see `LoginPanel` instead of routes. Authenticated pages then fetch Mastodon JSON in `useEffect`, hydrate `ViewCache`, and subscribe to user streaming. The router does not load data.

## What Is Already Typed, And What Is Not

`AppRoute` is a closed ADT with `fromPathname` / `toPath` / `label` / hub helpers. Navigation hubs use it. Most of the product does not.

`App.tsx` registers these routes, but `AppRoute` has no corresponding kind:

| Path | Page |
| --- | --- |
| `/status/:statusId` | thread |
| `/status/:statusId/history` | edit history |
| `/status/:statusId/favourited-by` | favouriters |
| `/status/:statusId/reblogged-by` | boosters |
| `/status/:statusId/quotes` | quotes |
| `/profile/:accountId` | other profiles |
| `/profile/:accountId/followers` | followers |
| `/profile/:accountId/following` | following |

Search also lives as `kind: "Search"` with no `q` / `type` fields, while `SearchPage` stores those values in the URL.

Call sites that bypass `AppRoute` include `StatusCard`, `NotificationCard`, `AccountRow`, `useStatusActions`, `useAppKeyboard`, `GoChord`, `MeHubNav`, `InboxTabs`, and `linkify-mentions.ts` (the last one emits a full `/app/search?q=` href because it writes HTML, not a React `Link`).

That split is the bug class a modern router is supposed to prevent: a new page can land in JSX and still be invisible to `fromPathname`, keyboard chords, and tests.

## What “Modern” Must Mean Here

The Worker, not the UI bundler, owns:

- `/api/*` Mastodon and streaming endpoints
- `/app/login` and `/app/logout` Auth0 redirects
- SPA fallback for `/app/*`
- Cache-Control for UI assets

A routing upgrade is a good fit only if it stays a **client history library** with typed paths, params, and search. It is a poor fit if it wants to own SSR, loaders that hit the origin as a JS server, or Vite plugin takeover of the Worker entrypoint.

Loaders and actions are also a weak match for current data flow:

- Home, notifications, and profiles already use `ViewCache` plus streaming patches.
- Timelines prefetch the next page from scroll, not from a route transition.
- Compose, confirm, and unread counts are session-scoped React context, not route data.

Keep that client cache. Do not rewrite it into router loaders as part of a first migration.

## Candidates

### 1. TanStack Router, SPA library mode — preferred

**Shape.** Replace `react-router` with `@tanstack/react-router`. Create a code-based route tree (`createRootRoute`, `createRoute`, `createRouter`) with `basepath: "/app"`. Wrap today’s providers on the root route. Keep `ui/pages/*` as page modules; do not force a `routes/` file tree on day one.

**Why it fits.**

- Path params (`statusId`, `accountId`, `tagName`, `conversationId`) become required typed fields instead of optional strings.
- Search state (`q`, `type`) can be validated instead of `URLSearchParams.get`.
- `Link` / `navigate` reject unknown paths at compile time, which collapses `AppRoute`, JSX routes, and string literals into one tree.
- Code splitting can move from handmade `React.lazy` to per-route `lazy` / auto-splitting without changing the Worker.
- Tests swap `MemoryRouter` for `createMemoryHistory` plus a test router. The `renderPage` helper stays.

**Why code-based first.** File-based routing wants a `src/routes` layout and generated `routeTree.gen.ts`. The current pages already sit under `ui/pages` with shared collection components. A generated file tree would be a second move. Code-based routing can encode the same tree the ADT already describes.

**Risks.**

- Every `Link` / `NavLink` / `useNavigate` / `useParams` / `useSearchParams` call site changes (about 26 files today).
- Custom `NavLink` active logic for inbox and “me” hubs must be re-expressed (`activeOptions`, `inactiveProps`, or a small wrapper around `AppRoute.isInboxPath` / `isMePath`).
- HTML mention links stay absolute `/app/search?...` hrefs; they cannot become typed `Link`s without a React renderer.

### 2. React Router 8 Data Mode — smaller step, weaker types

Stay on `react-router@8`, replace `BrowserRouter` + `<Routes>` with `createBrowserRouter` / `RouterProvider`, and attach `lazy()` on each route object.

**Gains.** Official lazy route loading, optional `errorElement`, future-friendly data APIs, same package.

**Limits.** Params and `href` are still mostly strings unless Framework Mode typegen is added. Search params stay manual. Loaders would duplicate `ViewCache` unless left unused. This modernizes the *API*, not the *contract*.

Use this only if a new dependency is unacceptable and the goal is “stop using Declarative Mode.”

### 3. React Router 8 Framework Mode — poor fit

Framework Mode adds a Vite plugin, route modules, `href` typegen, and SPA/SSR/SSG strategies. It wants to own the Vite app as a Remix-shaped framework.

That fights this repository:

- The HTTP server is Rust on Workers, not a JS server adapter.
- Auth0 login/logout and UI asset fallback already live in `crates/cfwdon-worker`.
- `wrangler.toml` `not_found_handling = "none"` is intentional; the Worker decides which `/app` paths are HTML.

SPA-only Framework Mode is possible in the abstract, but it buys a second framework next to the Worker for little type-safety gain over TanStack Router.

### 4. TanStack Start, Next.js, Remix — out of scope

These are full-stack JS servers. They would re-home rendering, data loading, and deploy shape. The Mastodon API and ActivityPub stack must stay on the Rust Worker. Do not introduce a second origin or a JS SSR runtime for `/app`.

### 5. Stay on Declarative Mode — acceptable only as a pause

Lowest churn. The package is already current. The incomplete `AppRoute` and string-literal paths remain. If routing work is deferred, still finish the ADT so hubs, keyboard chords, and status/profile links share one path helper.

## Recommendation

1. **Do not** move data fetching into router loaders, SSR, or a JS app framework.
2. **Do** treat the route tree as the single source of path strings, params, and search.
3. **Prefer TanStack Router** (SPA, code-based) as the library that can actually own that tree.
4. **Keep** `ViewCache`, streaming, session providers, `/app` basename, Worker login/logout, and SPA HTML fallback unchanged.
5. **Complete `AppRoute` first** so a later library swap is a mechanical adapter change instead of a treasure hunt for `"/status/${id}"`.

## Phased Spike

### Phase 0 — typed paths, same library

Expand `AppRoute` (or a successor ADT) to cover status, profile-by-id, collection, and search-query variants. Point `GoChord`, `MeHubNav`, `InboxTabs`, `StatusCard`, and `useStatusActions` at `AppRoute.toPath`. Keep React Router. This is useful even if the library never changes.

Acceptance:

- `fromPathname(toPath(route)) === route` for every kind, including `/status/:id` and `/profile/:id/followers`
- `route.test.ts` fails if `App.tsx` grows a path the ADT does not know (or the route table is generated from the ADT)

### Phase 1 — library spike on three routes

Spike TanStack Router for `/`, `/status/$statusId`, and `/search` only, behind a branch.

Prove:

- `basepath: "/app"` still matches Worker fallback and the service worker
- Root providers (session, view cache, compose, unread) still wrap the outlet
- Anonymous session still renders `LoginPanel` instead of app routes
- Typed `Link` to a thread and typed search `q` / `type`
- Vitest can mount those routes with memory history
- Bundle split for the thread page still works
- Mention HTML continues to land on `/app/search?q=`

Stop the spike if basename, session gating, or test harness cost more than the type-safety gain.

### Phase 2 — remaining routes

Migrate the rest of the table, then delete `react-router`. Re-express inbox/me active states. Keep `AppRoute` only if it still adds value as a domain helper (`label`, hub tests) sitting *on top of* the router tree; otherwise fold labels into route static data.

### Phase 3 — optional later

Typed search for more query flags, route-level pending UI, scroll restoration. Still no SSR. Still no moving Mastodon fetches into server loaders.

## Non-Goals

- Server-rendering the web UI
- Replacing `ViewCache` or streaming with router cache
- File-based `src/routes` layout as a prerequisite
- Changing `/app` URL space or Auth0 redirects
- Rewriting `admin-ui` (Svelte, no React Router)

## Impact Sketch

About 26 `web-ui` files import `react-router` today. Tests that wrap `MemoryRouter` (`render-page.tsx`, `useAppKeyboard.test.tsx`, `AccountRow.test.tsx`) must follow. Worker routing, `wrangler.toml` assets, and `admin-ui` stay out of scope.

`vite.config.ts` currently groups `react-router` into the `react-vendor` chunk. A TanStack swap should update that test regex so the vendor split stays one React family chunk.
