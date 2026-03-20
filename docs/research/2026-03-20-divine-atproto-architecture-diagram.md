# DiVine ATProto Architecture Diagram Description

> Status: supporting diagram input. The canonical source of truth is `docs/plans/2026-03-20-divine-atproto-unified-plan.md`.

## Diagram Goal

Show DiVine's publish path from mobile creation through Nostr relay storage and into ATProto distribution, with all trust boundaries and storage layers labeled.

## Mermaid System Diagram

```mermaid
flowchart LR
    A["DiVine Mobile App"] --> B["Nostr Signing Layer"]
    B --> C["funnelcake Relay"]
    A --> D["Blossom Media Upload"]
    D --> E["Blossom Object Storage"]
    V["login.divine.video"] --> F["DiVine AT Bridge"]
    V --> G["Account Link Store"]

    C --> F["DiVine AT Bridge"]
    F --> G["Account Link Store"]
    F --> H["Moderation Policy Engine"]
    F --> I["Replay / Offset Store"]
    F --> J["Video Worker"]

    J --> E
    J --> K["ATProto Blob Store (S3-compatible)"]
    F --> L["rsky-pds"]
    L --> M["PostgreSQL"]
    L --> K

    L --> N["ATProto Sync Endpoints"]
    N --> O["AT Relay Network / AppViews"]
    O --> P["Bluesky"]
    O --> Q["Skylight"]
    O --> R["Flashes / Other AT Clients"]

    H --> S["DiVine Labeler"]
    S --> O
    O --> H

    T["Gorse / Feed Ranking"] --> U["DiVine Feed Generator"]
    U --> O
```

## Render Notes

Use these labels in the finished visual:

- "Source of truth" on `funnelcake Relay`
- "Derived distribution path" on `DiVine AT Bridge`
- "Consent and account linking" on `login.divine.video`
- "User-signed Nostr event" on the edge from `Nostr Signing Layer` to `funnelcake Relay`
- "Media bytes" on the path through Blossom
- "Repo writes and blob refs" on the edge from `DiVine AT Bridge` to `rsky-pds`
- "Federated sync" on the path from `rsky-pds` to `AT Relay Network / AppViews`

## Sequence Diagram

```mermaid
sequenceDiagram
    participant User
    participant App as DiVine App
    participant Login as login.divine.video
    participant Relay as funnelcake
    participant Blossom as Blossom
    participant Bridge as AT Bridge
    participant Worker as Video Worker
    participant PDS as rsky-pds
    participant RelayNet as AT Relay/AppView
    participant Client as AT Clients

    User->>Login: opt in and link AT distribution
    Login->>Bridge: provision account link and consent state
    User->>App: record and publish 6-second loop
    App->>Blossom: upload media asset
    App->>Relay: publish signed NIP-71 event
    Bridge->>Relay: consume event stream
    Bridge->>Bridge: verify signature and consent
    Bridge->>Worker: fetch media and derive metadata
    Worker->>Blossom: read source blob
    Worker->>PDS: upload or preprocess video blob
    Bridge->>PDS: create feed.post with embed.video
    PDS->>RelayNet: expose repo commit
    RelayNet->>Client: deliver renderable AT post
```

## Storage Layer Notes

- Blossom is the origin store for DiVine media on the Nostr side.
- The ATProto blob store is the authoritative blob host for mirrored AT records.
- PostgreSQL stores PDS state and bridge mapping state.
- Replay and offset storage must be durable enough to resume after outages without republishing duplicates.

## Trust Boundaries

- User trust boundary: DiVine app and the user's Nostr signature
- DiVine operational boundary: funnelcake, Blossom, bridge workers, PDS, moderation services
- Federated boundary: AT relays, AppViews, and downstream clients

## Diagram Caption

DiVine keeps Nostr as the authoring and storage source of truth, then republishes verified video posts into ATProto through a DiVine-operated PDS so standard AT clients can render the content without relying on third-party bridges.
