# Divine ATProto Social Video App Design

**Date:** 2026-03-28
**Status:** Approved

## Purpose

Build a real Divine web product on AT Protocol infrastructure, not a companion viewer and not a thin frontend over the existing Nostr stack.

The product goal is a public-network social-video app that feels Divine-first while using ATProto-native identity, records, blobs, and social actions.

## Decision Summary

- Use `hatk` as the primary product app stack.
- Treat the public ATProto network as the default runtime environment.
- Keep the product video-first, but still support the standard social primitives needed for a real networked client.
- Use standard `app.bsky.*` records and standard OAuth/session flows instead of Divine-private protocol behavior.
- Keep any Nostr bridge or import path optional. The app must remain useful even if the Nostr side is completely disconnected.

## Why This Replaces The Viewer-Lab Direction

The earlier `divine-blacksky-viewer` and `divine-appview` work proved that Divine content can be rendered through ATProto-shaped read surfaces. That was a valid lab.

This project is different. The target is now a full Divine social-video application analogous to `divine-web`, except built as an ATProto-native product. That changes the boundary:

- the app is no longer a small read-only viewer
- the app is no longer Divine-PDS-only
- the app needs real sign-in, publishing, social actions, and public-network compatibility
- the app must stand on its own as a product, not as a demo for other backend services

## Product Principles

### Divine First

The app should look and behave like Divine, not like a reskinned generic ATProto client. The video experience, discovery model, creator presentation, and publishing flow should all be opinionated toward social video.

### ATProto Native

The app should use public-network identity and standard record writes wherever possible:

- `app.bsky.actor.profile`
- `app.bsky.feed.post`
- `app.bsky.feed.like`
- `app.bsky.feed.repost`
- `app.bsky.graph.follow`

The protocol should not be hidden behind Divine-only APIs for core client behavior.

### Public Network First

The app must work for broader public ATProto accounts, not only Divine-hosted accounts. Divine-hosted accounts can still be important for testing and quality control, but the product target is the public network.

### Video As The Product Wedge

The app is not a generic social client that happens to support videos. It is a social-video product that still supports the rest of the social graph.

## Scope

### Core V1 Surfaces

- `Home`
  - video-first feed
  - mix of following and discovery
- `Watch`
  - focused playback surface
  - caption, author, actions, thread entry points
- `Profile`
  - creator header
  - video-forward post list or grid
- `Thread`
  - conversation around a post
  - keep video prominent when present
- `Publish`
  - create a post with text plus video
  - allow text-only posts where needed
- `Search`
  - discover people and posts
  - bias toward video creators and clips

### Explicitly Deferred

- notifications as a full inbox surface
- moderation and labeling beyond what public ATProto clients already consume
- bespoke Divine-only record schemas for the core experience
- hard dependency on Divine-operated appview or feedgen services
- dependence on Nostr content as a prerequisite for the app to be usable

## Architecture

### Primary Application Stack

Create a new app at `apps/divine-atproto-web/` using `hatk` as the primary full-stack web framework.

This app owns:

- the product UI
- routing
- authenticated session state
- feed and custom XRPC handlers that are product-specific
- ATProto client-side and server-side interactions

`hatk` is a good fit because it bundles:

- a SvelteKit frontend
- typed XRPC handlers
- typed feed handlers
- server-side OAuth with encrypted session cookies
- typed record and blob helpers

### Runtime Model

The app should operate as a public ATProto client first:

- sign users in against their own PDS via OAuth
- read public-network actor and post data through standard ATProto read surfaces
- write standard records back to the authenticated user’s repo

The app can still expose Divine-specific feeds or helper endpoints, but the baseline experience should not require a Divine-only server cluster.

### Relationship To Existing Rust Services

Existing Rust services in this repo are no longer the primary product frontend path.

They now fall into three buckets:

1. `Keep as lab/support infrastructure`
   - `divine-appview`
   - `divine-appview-indexer`
   - `divine-feedgen`

2. `Keep as optional Divine-specific media or bridge utilities`
   - `divine-video-worker`
   - `divine-atbridge`

3. `Do not make core product correctness depend on them`
   - the app must still function as a public-network ATProto client if these services are absent

This keeps the ATProto product honest. If a Divine-specific backend service adds value, it should do so because it improves the product, not because the product cannot function without it.

## Read Behavior

### Default Read Path

For v1, the app should behave like a public ATProto client:

- fetch profiles, posts, and thread context from public-network read surfaces
- avoid making the existing Divine lab appview a hard dependency
- keep Divine-only indexing limited to additive features such as custom discovery or analytics

### Feed Model

V1 should ship with two core feed surfaces:

- `Following`
  - grounded in the user’s social graph
  - not Divine-specific
- `Divine Discovery`
  - Divine’s opinionated social-video discovery feed
  - product-defining differentiator

`Divine Discovery` can be implemented as a custom `hatk` feed generator or a thin adapter over a Divine-owned ranking service, but the contract returned to clients should still be ATProto feed items.

## Write Behavior

### Auth

Use OAuth and session cookies for the main web product. Logged-out browsing is allowed, but any write path requires an authenticated session.

If auth refresh fails, the app should degrade to logged-out mode instead of getting stuck in a broken session.

### Social Writes

Standard social actions should be normal ATProto record writes:

- like
- repost
- reply
- follow

Do not introduce Divine-only procedure endpoints for these actions.

### Video Publish Path

Publishing is where interoperability is most fragile, so the behavior needs to be explicit:

1. Check whether the authenticated host supports the target video publish flow.
2. Prefer native ATProto video publishing.
3. Upload the video through the user’s ATProto path.
4. Create a standard post record embedding the uploaded video.
5. Preserve publish draft state until the user gets a clear success or failure outcome.

If the host does not support the needed path cleanly, block publish with a concrete compatibility message. Do not silently fall back to a Divine-private media scheme.

## Divine-Specific Value

Divine should differentiate through product behavior, not private protocol forks.

The highest-value Divine-specific areas are:

- social-video discovery and ranking
- watch-first interface design
- creator-oriented profile presentation
- publish ergonomics for video-heavy posting
- optional interoperability with Divine’s broader ecosystem

The weakest place to differentiate is by inventing custom protocol behavior for core client functions.

## Failure Modes

### Auth Failure

- drop back to logged-out mode
- preserve any in-progress draft that is local to the browser
- show a concrete retry action

### Partial Read Data

- render degraded states cleanly
- show missing or delayed network data as loading or unavailable, not as silently absent

### Video Publish Failure

- keep the draft locally
- show the exact failing step
- never leave the user guessing whether a post partially published

### Host Capability Mismatch

- explicitly state that the current host does not support the required video flow
- do not invent a fallback that only works inside Divine

### Network Variability

- use skeleton states for feed and watch loads
- retry opportunistically
- keep UI usable under slower public-network conditions

## Testing And Validation

### Unit Coverage

- session helpers
- record builders
- feed ranking helpers
- publish state transitions
- compatibility detection helpers

### Integration Coverage

- OAuth login and logout
- profile fetch
- post and thread fetch
- video publish flow
- like, repost, reply, follow writes

### Compatibility Coverage

Test with:

- at least one Divine-hosted ATProto account
- at least one public-network ATProto account not hosted by Divine

### End-To-End Acceptance

The minimum product acceptance path is:

1. sign in
2. browse a feed
3. open a watch page
4. publish a video post
5. confirm the post is visible through public-network reads

## Delivery Strategy

This product is too broad for a single implementation plan. It should be delivered as phased, independently useful slices.

### Milestone 1: Thin Vertical Slice

Ship a real but narrow product slice:

- `hatk` app scaffolded in-repo
- public-network sign-in
- logged-out and logged-in home feed
- watch page for video posts
- creator profile page
- one end-to-end video publish flow
- one Divine discovery feed

This milestone proves the architecture and the product wedge.

### Milestone 2: Social Completeness

Add the missing core client behaviors:

- replies
- likes
- reposts
- follows
- better thread presentation
- stronger search

### Milestone 3: Product Depth

Improve the app as a social-video product:

- higher-quality discovery
- better creator tools and publish ergonomics
- richer watch experience
- notifications if still justified

### Milestone 4: Optional Ecosystem Integration

Only after the ATProto product stands on its own:

- optional Nostr import or bridge paths
- optional Divine-owned ranking or analytics services
- optional deeper integration with other Divine systems

## Repository Impact

This project adds a new first-class app under `apps/`.

When implementation begins, the repo docs must be updated alongside the code:

- `AGENTS.md`
- `README.md`
- `docs/runbooks/dev-bootstrap.md`
- a new runbook for the ATProto web app

## Success Criteria

The project is successful when:

- Divine has a usable social-video product running on ATProto infrastructure
- the app works with public-network ATProto accounts
- the product does not require the Nostr stack to stay online
- video publishing and viewing feel first-class
- the user experience feels unmistakably Divine rather than like a generic Bluesky clone
