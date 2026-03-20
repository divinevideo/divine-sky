# Keycast Bring-Your-Own ATProto With Custodial Nostr Design

> Status: approved conversational design captured from the 2026-03-20 architecture discussion.

## Goal

Let a user bring an existing ATProto account on their own PDS, authenticate to Keycast via ATProto, receive a fully custodial Nostr key managed by Keycast, claim a vanity NIP-05 alias under `*.bluesky.name`, and keep authored content synchronized across ATProto and Nostr including edits and deletes.

## Core Decisions

### 1. Existing ATProto Account, Not DiVine-Provisioned ATProto

Keycast does not provision a fresh ATProto identity for this flow.

- The user already has an ATProto account.
- The account may be hosted on the user's own PDS or another non-Keycast PDS.
- Keycast authenticates as a client of that existing account.
- Keycast must not attempt to host or validate the user's existing ATProto handle or PDS domain.

### 2. Canonical Identity Key Is The User's AT DID

The stable owner key for the linked account is the user's AT DID, normally `did:plc:...`.

- Ownership, authorization, and alias claims are bound to the AT DID.
- The current AT handle and current PDS endpoint are mutable metadata.
- Handle changes and PDS migration do not change the owning identity inside Keycast.

### 3. Login Happens On `login.divine.video`

`login.divine.video` acts as the Keycast control plane.

It is responsible for:

- ATProto login and token refresh lifecycle
- consent capture
- account status and disable flows
- alias search and claim
- support and recovery UI

It is not the signing boundary and should not hold raw Nostr private keys in the web application process.

### 4. Nostr Keys Are Fully Custodial

On first successful account link, Keycast creates a Nostr keypair for the linked AT DID.

- One hosted Nostr identity per linked AT DID
- private key stored encrypted at rest behind a dedicated signer boundary
- no end-user key export in v1
- no self-custody or recovery phrase in v1

This is intentionally custodial. Recovery is an account recovery flow, not a key recovery flow.

### 5. Public Nostr Identity Is A Vanity Alias Under `*.bluesky.name`

Keycast issues a human-facing NIP-05 alias such as:

- `alice-bsky-social.bluesky.name`

This alias is:

- a DID-owned claim in Keycast's database
- resolved via `https://alice-bsky-social.bluesky.name/.well-known/nostr.json?name=_`
- a Keycast-issued alias namespace, not proof that the user controls `bsky.social` or any external domain

The alias should be presented as a vanity identity, not as a validation of the user's original ATProto domain.

### 6. Alias Ownership Is Not A Pure Handle Transform

The visible alias may be suggested from the user's current AT handle, but it is not the canonical identity key and does not need to be a deterministic mathematical transform.

- The alias is claimed and reserved by AT DID.
- The suggested alias may use a readable domain stem such as `alice-bsky-social`.
- If a suggestion is unavailable, Keycast may offer another readable variation.

### 7. Keycast Is The Coordination Layer For Cross-Network Authoring

Keycast is the system coordinating writes and reconciliation across Nostr and ATProto.

- It stores the authoritative linkage between the hosted Nostr identity and the linked AT identity.
- It persists cross-network record mappings.
- It enforces the conflict policy and retry policy.

This does not require Keycast to redefine the user-facing source of truth. It does require Keycast to act as the authoritative sync coordinator.

### 8. Edits And Deletes Must Pass Through Both Ways

The linked identity is expected to remain synchronized across both networks.

- A create on one side must be projected to the other side.
- An edit on one side must be projected to the other side.
- A delete on one side must be projected to the other side.

For Nostr, Keycast uses the hosted key to emit the relevant replacement or deletion event for the mapped record.

## User Experience

### Link Flow

1. User logs into Keycast on `login.divine.video`.
2. User authenticates with their existing ATProto account.
3. Keycast stores:
   - AT DID
   - current AT handle
   - current PDS endpoint
   - delegated AT auth state
4. Keycast generates a custodial Nostr keypair for that AT DID.
5. User claims or accepts a suggested vanity alias under `*.bluesky.name`.
6. Keycast starts synchronizing authored content across ATProto and Nostr.

### Identity Presentation

The user keeps their existing ATProto identity exactly as it is today.

Keycast separately presents:

- existing AT handle: the user's AT identity
- `alice-bsky-social.bluesky.name`: the user's Keycast-issued Nostr alias

Those are related identities, but they are not the same namespace and should not be presented as if they prove control over one another.

## Service Boundaries

### 1. Control Plane

`login.divine.video`

- login
- token refresh
- consent
- alias claim
- disable and reconnect flows
- account support tooling

### 2. Signer

Dedicated signer boundary, separate from the web app process.

- generate hosted Nostr keypairs
- sign Nostr create, replace, and delete events
- enforce per-account disable state
- write audit records for every signature request

### 3. Sync Worker

Worker responsible for cross-network reconciliation.

- consume create, edit, and delete operations
- project AT changes to Nostr
- project Nostr changes to AT
- maintain idempotency and retry state

### 4. Alias Resolver

Host-based NIP-05 resolver for `*.bluesky.name`.

- serves `/.well-known/nostr.json`
- resolves `name=_` for the host
- returns the hosted Nostr pubkey for the active alias claim

## Data Model

At minimum the system needs the following durable records.

### `linked_accounts`

- `at_did`
- `current_at_handle`
- `current_pds_url`
- `at_access_token_ref`
- `at_refresh_token_ref`
- `nostr_pubkey`
- `nostr_key_ref`
- `status`
- `disabled_at`
- `created_at`
- `updated_at`

Primary logical owner key: `at_did`

### `alias_claims`

- `alias_host`
- `at_did`
- `is_primary`
- `status`
- `created_at`
- `updated_at`

The active alias host is unique. Ownership is bound to `at_did`.

### `content_mappings`

- `mapping_id`
- `at_did`
- `nostr_event_id`
- `nostr_kind`
- `nostr_d_tag`
- `at_uri`
- `at_rkey`
- `at_cid`
- `state`
- `created_at`
- `updated_at`

### `sync_operations`

- `operation_id`
- `at_did`
- `origin`
- `mapping_id`
- `operation_type`
- `payload_ref`
- `state`
- `attempt`
- `last_error`
- `last_applied_at`
- `created_at`
- `updated_at`

## Sync Model

### Create

When the user creates content through Keycast:

1. Keycast persists a pending sync operation.
2. Signer produces the Nostr event using the hosted key.
3. Keycast writes the mapped AT record to the user's PDS using delegated AT auth.
4. Keycast stores the resulting `nostr_event_id <-> at_uri/rkey/cid` mapping.
5. The operation becomes complete only after both sides have succeeded.

### Edit

When a linked record is edited on either side:

1. Keycast resolves the existing mapping.
2. Keycast creates a new sync operation.
3. Keycast applies the edit to the opposite side.
4. Keycast updates the mapping metadata and last-applied timestamps.

### Delete

When a linked record is deleted on either side:

1. Keycast resolves the existing mapping.
2. Keycast creates a delete operation.
3. Keycast emits the Nostr deletion or replacement event required for the hosted identity.
4. Keycast calls the appropriate ATProto delete against the user's PDS.
5. Keycast marks the mapping deleted once both sides have converged.

## Conflict Policy

Keycast is the authoritative sync coordinator.

The default conflict rule for v1 is:

- each incoming mutation becomes a durable sync operation
- operations are idempotent
- the latest successfully accepted operation wins
- conflicts are resolved by Keycast's recorded operation ordering, not by guessing from display names or mutable handles

This is intentionally operationally simple. If stronger CRDT-style semantics are needed later, they can be introduced after v1.

## Failure Policy

### Partial Success

If one side succeeds and the other fails:

- keep the mapping and operation in a retryable pending state
- do not emit duplicate user-visible records
- retry the failed side idempotently until success or terminal operator intervention

### AT Auth Failure

If delegated AT auth is expired or revoked:

- stop AT writes for that account
- mark sync operations blocked
- require reconnect on `login.divine.video`

### Account Disable

If the account is disabled:

- signer stops producing new Nostr signatures
- alias resolution stops returning active identity data
- sync workers stop processing new outbound operations for that account

## Security Requirements

- raw Nostr private keys must not live in the web app process
- signer requests must be authenticated and auditable
- every signature, delete, and alias change must produce an audit record
- token storage must use encrypted references, not plain-text application fields
- admin and support actions must be separately logged

## Non-Goals For V1

- proving ownership of the user's external AT handle domain
- hosting the user's original ATProto handle
- exporting the hosted Nostr private key
- supporting multiple active hosted Nostr identities per AT DID
- building a generalized multi-protocol identity graph

## Implementation Consequence For This Repository

This model is different from the current DiVine-hosted ATProto design already present in the repo.

The current code assumes:

- DiVine-provisioned AT accounts
- DiVine-owned AT handles
- DiVine-controlled PDS write path

The Keycast model described here should therefore be treated as a separate architecture track rather than a minor tweak to the current provisioning path.
