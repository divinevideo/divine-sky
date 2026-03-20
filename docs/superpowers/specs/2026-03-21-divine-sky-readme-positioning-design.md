# divine-sky README Positioning Design

## Goal

Create a top-level README for `divine-sky` that immediately answers the question: "Why does a Nostr project have an AT Protocol repo?"

## Audience

- curious external engineers
- protocol-native contributors from Nostr or ATProto
- teammates who need a fast explanation of repository intent

## Core Message

`divine-sky` is an active experiment. It exists because DiVine is Nostr-first, but the project wants to explore whether open social ecosystems grow stronger when protocols can interoperate instead of compete as sealed worlds.

## Content Shape

1. Open with a blunt explanation of what this repository is.
2. State clearly and early that the repo is experimental.
3. Explain why a Nostr project is building ATProto-side infrastructure.
4. Describe what lives in this repo in plain language.
5. Clarify what the repo is not, to prevent wrong assumptions.
6. Link readers to the canonical plan and runbooks if they want deeper operational detail.

## Tone

Technical manifesto, but grounded. The README should sound ambitious about open social protocol ecosystems without drifting into vague movement language.

## Constraints

- The README should be useful even for readers who do not know the internal DiVine architecture.
- It should stay accurate to the current workspace and canonical ATProto plan.
- It should remain light on setup details; operational material should mostly stay in runbooks.

## Success Criteria

- A confused outsider can read the first screen and understand why this repo exists.
- The README makes it unmistakable that `divine-sky` is an experiment, not a claim that DiVine is abandoning Nostr.
- The README gives enough architecture and repository shape to orient a contributor without turning into a setup manual.
