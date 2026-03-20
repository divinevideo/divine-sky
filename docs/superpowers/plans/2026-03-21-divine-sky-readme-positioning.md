# divine-sky README Positioning Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a top-level README that explains why `divine-sky` exists, frames it as an explicit experiment in cross-protocol growth, and gives readers a compact map of the repository.

**Architecture:** This is a documentation-only change. The implementation creates a new root `README.md` with a narrative-first structure, then verifies that the referenced files and commands align with the current repository state.

**Tech Stack:** Markdown, Cargo workspace metadata, repository docs

---

## Chunk 1: README Creation

### Task 1: Draft the repository framing

**Files:**
- Create: `README.md`
- Verify: `docs/plans/2026-03-20-divine-atproto-unified-plan.md`
- Verify: `docs/runbooks/dev-bootstrap.md`
- Verify: `docs/runbooks/source-of-truth.md`

- [ ] **Step 1: Write the README**

Add sections covering:
- what `divine-sky` is
- why a Nostr project has an ATProto repo
- an explicit experimental-status warning
- a plain-language workspace map
- what the repo is not
- links to canonical docs

- [ ] **Step 2: Verify referenced material still exists**

Run: `test -f README.md && test -f docs/plans/2026-03-20-divine-atproto-unified-plan.md && test -f docs/runbooks/dev-bootstrap.md && test -f docs/runbooks/source-of-truth.md`
Expected: exit code `0`

- [ ] **Step 3: Review the README against the design**

Run: `sed -n '1,260p' README.md`
Expected: the README clearly answers the "why does this repo exist?" question in the opening sections and explicitly says the repo is experimental.

- [ ] **Step 4: Inspect the final diff**

Run: `git diff -- README.md docs/superpowers/specs/2026-03-21-divine-sky-readme-positioning-design.md docs/superpowers/plans/2026-03-21-divine-sky-readme-positioning.md`
Expected: only the new README and design/plan records appear.
