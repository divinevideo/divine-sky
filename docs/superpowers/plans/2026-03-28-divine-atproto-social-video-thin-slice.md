# Divine ATProto Social Video Thin Slice Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first production-shaped ATProto-native Divine web app slice: sign in, browse a video-first feed, open a watch page, inspect a creator profile, and publish a standard ATProto video post.

**Architecture:** Create a standalone `hatk` repository rooted at `divine-atproto-web/` as the main product surface. Treat the public ATProto network as the default runtime, keep Divine-only ranking as an additive custom feed, and use standard `app.bsky.*` records plus OAuth-backed sessions for writes.

**Tech Stack:** `hatk`, SvelteKit, TypeScript, Vitest, ATProto OAuth, standard `app.bsky.*` records, ATProto blob upload, Vite+

---

## File Structure

### New App

- Create: `app/routes/+layout.server.ts`
- Create: `app/routes/+layout.svelte`
- Create: `app/routes/+page.server.ts`
- Create: `app/routes/+page.svelte`
- Create: `app/routes/watch/[did]/[rkey]/+page.server.ts`
- Create: `app/routes/watch/[did]/[rkey]/+page.svelte`
- Create: `app/routes/profile/[actor]/+page.server.ts`
- Create: `app/routes/profile/[actor]/+page.svelte`
- Create: `app/routes/publish/+page.server.ts`
- Create: `app/routes/publish/+page.svelte`
- Create: `app/lib/atproto.ts`
- Create: `app/lib/ui/`
- Create: `app/lib/publish.ts`
- Create: `app/app.css`
- Create: `server/feeds/divine-discovery.ts`
- Create: `server/xrpc/dev.divine.getPublishSupport.ts`
- Create: `lexicons/dev/divine/getPublishSupport.json`
- Create: `test/`

### Repo Docs

- Modify: `README.md`
- Modify: `AGENTS.md`
- Modify: `docs/runbooks/dev-bootstrap.md`
- Create: `docs/runbooks/divine-atproto-web.md`

### Likely Generated Files

- Create: `hatk.config.ts`
- Create: `hatk.generated.ts`
- Create: `hatk.generated.client.ts`
- Create: `svelte.config.js`
- Create: `vite.config.ts`
- Create: `package.json`
- Create: `tsconfig.json`
- Create: `tsconfig.server.json`
- Create: `docker-compose.yml`

---

## Chunk 1: Scaffold The New `hatk` App

### Task 1: Bootstrap The App And Verify The Template Runs

**Files:**
- Create: `/Users/rabble/code/divine/divine-atproto-web/` from the `hatk` starter template
- Verify: `package.json`
- Verify: `hatk.config.ts`
- Verify: `svelte.config.js`

- [ ] **Step 1: Install Vite+ if it is missing**

Run:

```bash
vp help
```

Expected: help output. If `vp` is missing, install it:

```bash
curl -fsSL https://vite.plus | bash
vp help
```

- [ ] **Step 2: Scaffold the app from the official `hatk` starter**

Run:

```bash
cd /Users/rabble/code/divine
vp create github:hatk-dev/hatk-template-starter divine-atproto-web
```

Expected: a new `/Users/rabble/code/divine/divine-atproto-web/` repo containing `app/`, `server/`, `lexicons/`, `hatk.config.ts`, generated type files, and local docker bootstrap files.

- [ ] **Step 3: Sync SvelteKit and install dependencies**

Run:

```bash
cd /Users/rabble/code/divine/divine-atproto-web
npx svelte-kit sync
npm install
```

Expected: dependency install completes and generated SvelteKit files are current.

- [ ] **Step 4: Start the stock app once before changing behavior**

Run:

```bash
cd /Users/rabble/code/divine/divine-atproto-web
vp dev
```

Expected: the starter app runs locally on `http://127.0.0.1:3000` with the default seeded flow.

- [ ] **Step 5: Commit the clean scaffold**

```bash
git add .
git commit -m "feat: scaffold hatk app for divine atproto web"
```

### Task 2: Strip The Starter Into Divine’s Thin-Slice Shape

**Files:**
- Modify: `app/routes/+page.svelte`
- Modify: `app/routes/+page.server.ts`
- Modify: `app/routes/+layout.server.ts`
- Modify: `app/routes/+layout.svelte`
- Modify: `app/app.css`
- Delete or replace any starter-only demo route and seed code that does not serve the thin slice

- [ ] **Step 1: Write a failing test for the Divine shell**

Create a test under `test/root.test.ts` that starts the app with `startTestServer()` and asserts:

- the root page contains Divine branding
- the root page links to feed, publish, and profile surfaces
- the root page does not contain starter-demo copy

- [ ] **Step 2: Run the new test and verify it fails**

Run:

```bash
cd /Users/rabble/code/divine/divine-atproto-web
npx vitest run test/root.test.ts
```

Expected: FAIL because the starter app still renders its template UI.

- [ ] **Step 3: Replace the starter shell with a Divine shell**

Implement:

- session parsing in `+layout.server.ts`
- a Divine-branded layout in `+layout.svelte`
- a minimal home route shell in `+page.svelte`
- route-level load logic in `+page.server.ts`
- a product-specific stylesheet in `app.css`

- [ ] **Step 4: Re-run the root test**

Run:

```bash
npx vitest run test/root.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit the shell**

```bash
git add .
git commit -m "feat: replace hatk starter shell with divine app shell"
```

---

## Chunk 2: Public Read Flow For Home, Watch, And Profile

### Task 3: Add Shared ATProto Read Helpers

**Files:**
- Create: `app/lib/atproto.ts`
- Test: `test/atproto-read.test.ts`

- [ ] **Step 1: Write a failing test for the shared read helper**

Test the helper boundary, not the network:

- actor/profile lookup accepts a handle or DID
- post URI parsing for watch routes is normalized in one place
- thread fetch helpers produce typed data expected by routes
- failures normalize to predictable UI-safe errors

- [ ] **Step 2: Run the helper test**

Run:

```bash
npx vitest run test/atproto-read.test.ts
```

Expected: FAIL because the helper module does not exist yet.

- [ ] **Step 3: Implement the read helper module**

Implement focused helpers for:

- resolving actors
- fetching profiles
- fetching posts and thread data
- mapping public-network responses into the UI shape used by the routes

Keep this file focused on network contract translation. Do not bury route logic in it.

- [ ] **Step 4: Re-run the helper test**

Run:

```bash
npx vitest run test/atproto-read.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add .
git commit -m "feat: add shared atproto read helpers"
```

### Task 4: Build The Home Feed And Watch Page

**Files:**
- Modify: `app/routes/+page.server.ts`
- Modify: `app/routes/+page.svelte`
- Create: `app/routes/watch/[did]/[rkey]/+page.server.ts`
- Create: `app/routes/watch/[did]/[rkey]/+page.svelte`
- Create: `app/lib/ui/feed-card.svelte`
- Create: `app/lib/ui/video-player.svelte`
- Test: `test/feed-and-watch.test.ts`

- [ ] **Step 1: Write failing route tests**

Add tests that verify:

- the home route returns a video-first list shape
- the watch route renders a selected post, author metadata, and action affordances
- non-video posts degrade gracefully instead of crashing the watch page

- [ ] **Step 2: Run the route tests**

Run:

```bash
npx vitest run test/feed-and-watch.test.ts
```

Expected: FAIL.

- [ ] **Step 3: Implement the home route**

The home route should:

- load a primary feed source
- render video-first cards
- keep text and author metadata secondary

Use a narrow, explicit data contract between the route loader and the Svelte components.

- [ ] **Step 4: Implement the watch route**

The watch route should:

- accept a post identity from `[did]/[rkey]`
- fetch the post and surrounding thread context
- keep the video as the focal element
- show clear failure states when the record is missing or not playable

- [ ] **Step 5: Re-run the route tests**

Run:

```bash
npx vitest run test/feed-and-watch.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add .
git commit -m "feat: add divine feed and watch routes"
```

### Task 5: Build The Creator Profile Route

**Files:**
- Create: `app/routes/profile/[actor]/+page.server.ts`
- Create: `app/routes/profile/[actor]/+page.svelte`
- Create: `app/lib/ui/profile-header.svelte`
- Test: `test/profile.test.ts`

- [ ] **Step 1: Write the failing profile test**

The test should verify:

- actor resolution by handle or DID
- profile header data renders
- posts are rendered with video-forward treatment

- [ ] **Step 2: Run the profile test**

Run:

```bash
npx vitest run test/profile.test.ts
```

Expected: FAIL.

- [ ] **Step 3: Implement the profile route**

Implement:

- loader for actor + profile + recent posts
- profile header component
- list of recent posts with watch links

- [ ] **Step 4: Re-run the profile test**

Run:

```bash
npx vitest run test/profile.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add .
git commit -m "feat: add creator profile route"
```

---

## Chunk 3: OAuth Session And Divine Discovery Feed

### Task 6: Wire Real Session State Into The Layout

**Files:**
- Modify: `app/routes/+layout.server.ts`
- Modify: `app/routes/+layout.svelte`
- Test: `test/auth-layout.test.ts`

- [ ] **Step 1: Write the failing auth-layout test**

Verify:

- logged-out mode shows a clear sign-in path
- logged-in mode shows viewer identity
- auth failure falls back to logged-out mode without crashing

- [ ] **Step 2: Run the auth-layout test**

Run:

```bash
npx vitest run test/auth-layout.test.ts
```

Expected: FAIL.

- [ ] **Step 3: Implement session parsing and UI states**

Use the `hatk` auth flow in the app layout to:

- parse the viewer from cookies
- expose viewer state to all routes
- render login/logout affordances

Keep auth UI thin. The product work is in the feed and publish paths.

- [ ] **Step 4: Re-run the auth-layout test**

Run:

```bash
npx vitest run test/auth-layout.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add .
git commit -m "feat: add oauth-backed session layout"
```

### Task 7: Add The Divine Discovery Feed

**Files:**
- Create: `server/feeds/divine-discovery.ts`
- Create or modify: related generated test in `test/`
- Modify: `app/routes/+page.server.ts`

- [ ] **Step 1: Generate the feed scaffold**

Run:

```bash
cd /Users/rabble/code/divine/divine-atproto-web
npx hatk generate feed divine-discovery
```

Expected: `server/feeds/divine-discovery.ts` plus a corresponding test file.

- [ ] **Step 2: Write the failing feed behavior test**

Verify:

- the feed returns items in video-first order
- non-video records are either omitted or intentionally deprioritized
- anonymous and authenticated viewers both receive a usable feed

- [ ] **Step 3: Run the feed test**

Run:

```bash
npx vitest run test/divine-discovery*.test.ts
```

Expected: FAIL.

- [ ] **Step 4: Implement the feed ranking logic**

Keep the first version simple:

- prefer posts with a video embed
- sort by recency within that subset
- add a clear seam for future ranking work

Do not build a complex recommendation engine in the first slice.

- [ ] **Step 5: Re-run the feed test**

Run:

```bash
npx vitest run test/divine-discovery*.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add .
git commit -m "feat: add divine discovery feed"
```

---

## Chunk 4: Video Publish Flow

### Task 8: Add Publish Capability Detection

**Files:**
- Create: `lexicons/dev/divine/getPublishSupport.json`
- Create: `server/xrpc/dev.divine.getPublishSupport.ts`
- Test: `test/publish-support.test.ts`

- [ ] **Step 1: Generate the XRPC handler scaffold**

Run:

```bash
cd /Users/rabble/code/divine/divine-atproto-web
npx hatk generate xrpc dev.divine.getPublishSupport
```

Expected: the lexicon and handler files exist, plus a generated test stub.

- [ ] **Step 2: Write the failing capability test**

Verify the endpoint returns:

- authenticated vs anonymous status
- whether the current viewer has a publishable repo context
- whether the current host path is considered video-capable
- a user-facing reason string when publish is blocked

- [ ] **Step 3: Run the capability test**

Run:

```bash
npx vitest run test/publish-support*.test.ts
```

Expected: FAIL.

- [ ] **Step 4: Implement the capability endpoint**

Keep this endpoint narrow. It should report capability, not perform publish.

- [ ] **Step 5: Re-run the capability test**

Run:

```bash
npx vitest run test/publish-support*.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add .
git commit -m "feat: add publish capability endpoint"
```

### Task 9: Implement The Publish Route And Draft State

**Files:**
- Create: `app/routes/publish/+page.server.ts`
- Create: `app/routes/publish/+page.svelte`
- Create: `app/lib/publish.ts`
- Create: `app/lib/ui/publish-form.svelte`
- Test: `test/publish-route.test.ts`

- [ ] **Step 1: Write the failing publish route test**

Verify:

- anonymous viewers cannot publish
- authenticated viewers can create a draft
- incompatible hosts see a specific blocked state
- submit attempts preserve draft text on failure

- [ ] **Step 2: Run the publish route test**

Run:

```bash
npx vitest run test/publish-route.test.ts
```

Expected: FAIL.

- [ ] **Step 3: Implement route loaders and actions**

Implement:

- loader logic that calls the publish support endpoint
- route actions for draft submission
- UI state that clearly distinguishes ready, blocked, uploading, and failed

- [ ] **Step 4: Re-run the publish route test**

Run:

```bash
npx vitest run test/publish-route.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add .
git commit -m "feat: add publish route and draft states"
```

### Task 10: Implement Video Upload And Post Creation

**Files:**
- Modify: `app/lib/publish.ts`
- Test: `test/video-publish.test.ts`

- [ ] **Step 1: Write the failing publish integration test**

Use `startTestServer()` and assert:

- the app uploads a blob through the user path
- a standard record is created for the authenticated repo
- publish returns a success payload containing the resulting record URI

- [ ] **Step 2: Run the publish integration test**

Run:

```bash
npx vitest run test/video-publish.test.ts
```

Expected: FAIL.

- [ ] **Step 3: Implement the minimal end-to-end publish helper**

Implement the smallest possible flow:

1. validate that a file is present
2. upload it through the authenticated ATProto blob path
3. create a standard post record referencing the uploaded blob
4. return the resulting URI and CID

Do not add background transcoding, retries, or multiphase resumable upload logic in this slice unless tests force it.

- [ ] **Step 4: Re-run the publish integration test**

Run:

```bash
npx vitest run test/video-publish.test.ts
```

Expected: PASS.

- [ ] **Step 5: Manually verify the publish path**

Run:

```bash
cd /Users/rabble/code/divine/divine-atproto-web
vp dev
```

Manual check:

1. sign in with a test account
2. open `/publish`
3. attach a small video
4. submit the post
5. open the resulting watch page

Expected: the created post is visible in the app and on a public-network read path.

- [ ] **Step 6: Commit**

```bash
git add .
git commit -m "feat: publish video posts from divine atproto web"
```

---

## Chunk 5: Repo Docs And Verification

### Task 11: Document The New App In The Repo

**Files:**
- Modify: `README.md`
- Modify: `AGENTS.md`
- Modify: `docs/runbooks/dev-bootstrap.md`
- Create: `docs/runbooks/divine-atproto-web.md`

- [ ] **Step 1: Write the failing docs checklist**

Create a simple local checklist in the PR notes or commit notes covering:

- new app path documented
- standalone repo purpose documented
- local bootstrap documented
- auth expectations documented
- core commands documented

- [ ] **Step 2: Update the repo-facing docs**

Document:

- the repo purpose
- bootstrap commands
- expected local ports
- how to run the app
- the relationship between this standalone repo and the older `divine-sky` viewer-lab work

- [ ] **Step 3: Commit the docs**

```bash
git add README.md AGENTS.md docs/runbooks/dev-bootstrap.md docs/runbooks/divine-atproto-web.md
git commit -m "docs: add divine atproto web runbook and repo guidance"
```

### Task 12: Run Final Verification

**Files:**
- No code changes expected

- [ ] **Step 1: Run the app test suite**

Run:

```bash
cd /Users/rabble/code/divine/divine-atproto-web
npx vitest run
```

Expected: all app tests pass.

- [ ] **Step 2: Run a production build**

Run:

```bash
cd /Users/rabble/code/divine/divine-atproto-web
npx svelte-kit sync
npm run build
```

Expected: successful build with no type or route errors.

- [ ] **Step 3: Run one final local smoke pass**

Run:

```bash
cd /Users/rabble/code/divine/divine-atproto-web
vp dev
```

Manual check:

1. open the local app
2. verify the home feed renders
3. verify sign-in appears
4. open a watch route
5. verify publish route loads

Expected: the thin slice is navigable end to end.

- [ ] **Step 4: Final commit if needed**

```bash
git status --short
```

Expected: clean working tree or only intentional follow-up notes.

---

## Follow-On Plans

This thin-slice plan intentionally stops after:

- public-network sign-in
- feed
- watch
- profile
- one Divine discovery feed
- one end-to-end publish flow

The next plans should cover:

1. replies, likes, reposts, follows, and thread depth
2. search and notifications
3. creator tooling and richer discovery
4. optional Nostr interoperability after the ATProto app is independently strong

Plan complete and saved to `docs/superpowers/plans/2026-03-28-divine-atproto-social-video-thin-slice.md`. Ready to execute.
