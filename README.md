# divine-sky

`divine-sky` is the part of DiVine where a Nostr-native project experiments with AT Protocol infrastructure.

If your reaction is "what the hell is a Nostr project doing with an ATProto repo?", the short answer is: DiVine is trying to see whether open social protocols can grow each other instead of pretending only one of them should win.

This repository exists to test that idea in code.

## This Is Experimental

This repository is very clearly an experiment.

It is not a polished platform, not a finished product surface, and not a final statement of DiVine architecture. Some parts are production-shaped because experiments need real systems, but the point of `divine-sky` is still to learn, revise, and sometimes throw assumptions away.

If you are reading this as a signal that DiVine is abandoning Nostr, that is the wrong read. The experiment is about interoperability, distribution, identity, moderation, and protocol growth across ecosystems, not replacing one social protocol with another.

## Why Does A Nostr Project Have This Repo?

DiVine is still fundamentally Nostr-first in how it thinks about authorship and source-of-truth. But being Nostr-first does not require being Nostr-only.

`divine-sky` exists because there is a serious question worth testing: can a project born in one open protocol help content, identity, moderation signals, and social reach move into another open protocol without collapsing into a closed platform in the middle?

For DiVine, that means exploring things like:

- ATProto account provisioning for DiVine users
- handle and DID management
- republishing Nostr-native content into ATProto-compatible surfaces
- moderation and label translation across protocol boundaries
- feed generation and discovery experiments for video-native social products

The point is not "Nostr failed, so build ATProto stuff now."

The point is that open social ecosystems get stronger when projects are willing to build bridges, test real interoperability, and let users reach more than one network without forcing them to start over from scratch.

## What Lives Here

This repository is an ATProto-side companion workspace, not the entirety of DiVine.

Today it includes Rust crates for:

- `divine-atbridge`: bridging, replay, publish-path, and provisioning runtime work
- `divine-handle-gateway`: opt-in, export, disable, status, and handle-facing HTTP flows
- `divine-feedgen`: ATProto feed-generation experiments
- `divine-labeler`: label and moderation infrastructure
- `divine-moderation-adapter`: translation between moderation domains
- `divine-video-worker`: media-path processing for video-centric flows
- shared types and database support in `divine-bridge-types` and `divine-bridge-db`

It also carries runbooks, research notes, staging and deployment documents, and the implementation plans around DiVine's ATProto work.

## What This Repo Is Not

This repository is not:

- the whole DiVine product
- proof that DiVine has stopped being a Nostr project
- a generic ATProto starter kit
- a turnkey bridge for every protocol
- a claim that the current architecture is final

It is a focused experimental workspace for testing whether a Nostr-native system can responsibly participate in and contribute to the broader open social world.

## How To Read This Repo

If you want the canonical ATProto direction, start with:

- `docs/plans/2026-03-20-divine-atproto-unified-plan.md`

If you want the rule for which documents are canonical versus supporting material, read:

- `docs/runbooks/source-of-truth.md`

If you want operational setup details instead of the high-level framing in this README, start with:

- `docs/runbooks/dev-bootstrap.md`
- `docs/runbooks/launch-checklist.md`
- `docs/runbooks/atproto-opt-in-smoke-test.md`

## Local Development Paths

There are now two local development paths in this repository:

- `config/docker-compose.yml` is the fast default for day-to-day bridge and runtime work.
- `deploy/localnet/` is the fuller ATProto localnet lab for provisioning and protocol testing across PLC, PDS, Jetstream, DNS, and local handle administration.

The localnet profile is additive. It does not replace the fast stack, and it keeps local handle testing on `*.divine.test` instead of pretending the production public domain exists on a laptop.

## Why The Name Matters

The name `divine-sky` is a useful clue.

This repo sits in the overlap between DiVine's Nostr-native world and the ATProto "sky" ecosystem. It is where DiVine tests whether publishing, identity, moderation, and discovery can cross that boundary in a way that keeps protocols open and users legible to themselves.

That is the real purpose of this repository: not protocol tourism, but applied experimentation in how open social ecosystems might actually grow together.
