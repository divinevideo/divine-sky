# Chunk E — Resolve ATProto Rollout Gating

> **Sub-plan of** `docs/superpowers/plans/2026-05-30-atproto-production-rollout.md` (Chunk E, blocker #6).
> **Primary repo:** `divine-sky`. **Editability:** runbook edits land in `divine-sky`; sibling repos (`divine-web`, `divine-mobile`) are **read-only reference** — this chunk does not change them.
> **For agentic workers:** Use `superpowers:executing-plans`. Steps use checkbox (`- [ ]`) syntax. Every command below has an expected output; run it and compare before checking the box.

---

## Goal

Decide and document the real ATProto crosspost rollout gate. The master plan says two client flags — web `enableAtprotoPublishing` and mobile `FeatureFlag.atprotoPublishing` — "do not exist" and must be killed or built. A direct cross-repo audit (2026-05-30) found the picture is **more precise than that**, and the difference changes the runbook edits from "three deletions" to "one rename + one removal + one reword." This chunk records the verified state, recommends **Option A (document the real gate, fix the phantom language)**, and lists the exact two divine-sky runbook files/lines to change.

---

## Verified Ground Truth (read before editing — do not re-derive)

All three claims below were confirmed by reading source on 2026-05-30. Re-run the verification commands in Task E1 if you doubt any of them.

### 1. The authoritative gate is server-side: `crosspost_enabled && ready`

`crates/divine-atbridge/src/runtime.rs:64-76` — `account_link_from_lifecycle_row`:

```rust
let is_ready = row.provisioning_state == "ready";
if !is_ready || row.disabled_at.is_some() || !row.crosspost_enabled {
    return None;            // <- no AccountLink => bridge will not publish
}
```

If this returns `None`, the bridge has no `AccountLink` and publishes nothing for that pubkey. This is enforced regardless of any client UI. It is the gate that actually controls whether a creator's videos mirror to Bluesky. The launch checklist already documents it correctly at `docs/runbooks/launch-checklist.md:80` ("divine-atbridge only publishes when `crosspost_enabled && ready`").

### 2. Web flag `enableAtprotoPublishing` is GENUINELY PHANTOM

- `grep -rni "enableAtprotoPublishing\|atprotoPublishing" /Users/rabble/code/divine/divine-web/src` → no matches.
- `divine-web` has no ATProto crosspost/publishing UI surface at all. The only `bluesky` references in `divine-web/src` are (a) social footer links (`components/SocialLinks.tsx`) and (b) NIP-39 external-identity verification (`hooks/useExternalIdentities.ts`, `pages/LinkedAccountsSettingsPage.tsx`) — neither is ATProto crosspost publishing.
- The human ATProto opt-in console lives in **keycast** (`login.divine.video` → `settings/security` → `Bluesky Account` card), per `docs/runbooks/atproto-opt-in-smoke-test.md:62-68`. It is not a divine-web feature.
- **Conclusion:** there is no web flag and no web surface to gate. The runbook line referencing a web flag describes something that does not exist.

### 3. Mobile flag is NOT phantom — it exists under a DIFFERENT name

The master plan's name `FeatureFlag.atprotoPublishing` does not exist, but a real flag does:

- `divine-mobile/mobile/lib/features/feature_flags/models/feature_flag.dart` defines `blueskyPublishing` — display name `'Bluesky Publishing'`, description `'Enable Bluesky crosspost toggle in settings'`.
- Default is **off**: `build_configuration.dart:35-36` → `bool.fromEnvironment('FF_BLUESKY_PUBLISHING')` with no `defaultValue`, so it defaults to `false`.
- It gates real UI: `general_settings_screen.dart:31` reads `isFeatureEnabledProvider(FeatureFlag.blueskyPublishing)` to show/hide the crosspost toggle.
- **Conclusion:** the mobile control the runbook wants ALREADY EXISTS and already defaults off. The runbook simply has the wrong flag name.

### Net effect on the decision

```
                        master plan said        verified reality
web  enableAtprotoPublishing   phantom    →   phantom (no surface, nothing to gate)
mobile FeatureFlag.atproto...  phantom    →   EXISTS as blueskyPublishing (off by default, gates the toggle)
real publishing gate           —          →   crosspost_enabled && ready (divine-atbridge runtime.rs)
```

The mobile gate Option B would "build" is already built (under a different name); the web gate Option B would build has no surface to attach to. So Option B is redundant work with no benefit. This **strengthens** the master plan's Option-A recommendation.

---

## Decision: Option A (recommended) vs Option B

### Option A — Document the real gate, fix the phantom language (RECOMMENDED)

- Keep the authoritative gate where it is: server-side `crosspost_enabled && ready` in `divine-atbridge`, plus cohorted opt-in.
- Correct the mobile runbook line to the **real** flag name (`blueskyPublishing`, `FF_BLUESKY_PUBLISHING`, default off).
- Remove the web flag line (no web surface exists) and state that web has no ATProto publishing control — opt-in is in keycast.
- Reword the opt-in smoke-test assertion so it describes the real client surfaces (mobile flag) and the real server gate, instead of asserting a phantom web/mobile flag pair.

**Why A:** The gate that actually prevents publishing is server-side and already enforced + tested (Chunk G of the master plan; `crates/divine-atbridge/tests/bridge_opt_in_gate.rs`). The one real client surface (mobile) already has a flag that already defaults off. There is nothing to build; there is documentation to correct. Inventing matching client flags adds a second, weaker, easily-bypassed gate and more confusion.

### Option B — Build real client gates (NOT recommended)

Would add a web `enableAtprotoPublishing` flag (and wire it into a settings surface that does not exist yet) and rename/duplicate the mobile flag to match the runbook text. Rejected because:

- **Mobile half is already done** under the name `blueskyPublishing` — building `atprotoPublishing` is a redundant rename for cosmetic name-matching.
- **Web half has no surface** — there is no ATProto publishing UI in divine-web to gate; you'd be building a flag, then building a feature to put behind it, which is out of scope for a rollout-gating chunk.
- A client flag is **not a security boundary**: a third-party Bluesky client or a custom mobile build can ignore it. The server-side `crosspost_enabled && ready` gate is the real control; client flags only hide UI.

**Choose Option A.**

---

## Task E1 — Verify the ground truth (one-time, ~2 min)

Run these from `/Users/rabble/code/divine/divine-sky`. They confirm the three claims above before you edit anything.

- [ ] **Step 1: Confirm the server-side gate exists**

```bash
sed -n '64,76p' crates/divine-atbridge/src/runtime.rs
```
Expected: the `is_ready` / `!row.crosspost_enabled` early-return block shown above.

- [ ] **Step 2: Confirm the web flag is phantom and web has no ATProto publishing surface**

```bash
grep -rni "enableAtprotoPublishing\|atprotoPublishing" /Users/rabble/code/divine/divine-web/src || echo "PHANTOM-CONFIRMED"
```
Expected: `PHANTOM-CONFIRMED`. (Bluesky social-link / external-identity hits are unrelated and live elsewhere; this grep targets the flag name specifically.)

- [ ] **Step 3: Confirm the mobile flag exists under the real name and defaults off**

```bash
grep -n "blueskyPublishing\|FF_BLUESKY_PUBLISHING" \
  /Users/rabble/code/divine/divine-mobile/mobile/lib/features/feature_flags/models/feature_flag.dart \
  /Users/rabble/code/divine/divine-mobile/mobile/lib/features/feature_flags/services/build_configuration.dart \
  /Users/rabble/code/divine/divine-mobile/mobile/lib/screens/settings/general_settings_screen.dart
```
Expected: the enum entry, the `bool.fromEnvironment('FF_BLUESKY_PUBLISHING')` default (no `defaultValue` → off), and the `general_settings_screen.dart:31` gate.

- [ ] **Step 4: Confirm the phantom names appear in exactly two runbooks**

```bash
grep -rln "enableAtprotoPublishing\|FeatureFlag.atprotoPublishing\|atprotoPublishing" docs/runbooks/
```
Expected: only `docs/runbooks/launch-checklist.md`. (The `atproto-opt-in-smoke-test.md` line is a generic "client feature flags must be required" assertion that names no flag — it is still in scope to reword; see Task E3.) The other matches in `docs/superpowers/plans/2026-04-03-*` and `2026-03-20-*` are **historical plan files, not runbooks, and are OUT of scope** — do not edit them.

---

## Task E2 — Fix the phantom flag language in `launch-checklist.md`

**File (edit, divine-sky):** `docs/runbooks/launch-checklist.md` — the `## Rollout Controls` block, lines 39-40.

Current text:

```
- Keep `FeatureFlag.atprotoPublishing` off in mobile by default until backend verification is complete.
- Keep `enableAtprotoPublishing` off in web by default until rollout is explicitly enabled.
```

- [ ] **Step 1: Rename the mobile line to the real flag.** Replace line 39 with:

```
- Keep the mobile `FeatureFlag.blueskyPublishing` flag (env `FF_BLUESKY_PUBLISHING`, defaults off) off until backend verification is complete; it gates the Bluesky crosspost toggle in mobile settings but is NOT the publishing gate — the bridge still enforces `crosspost_enabled && ready` server-side.
```

- [ ] **Step 2: Replace the phantom web line.** Replace line 40 with:

```
- There is no web ATProto publishing flag or surface: `divine-web` has no crosspost UI; the human opt-in lives in the keycast console (`login.divine.video` → `settings/security`). Do not add a `enableAtprotoPublishing` web flag — gate rollout server-side instead.
```

- [ ] **Step 3: Add an authoritative-gate note** immediately under the (now corrected) two lines, so the checklist states the real control once, explicitly. Insert after the web line:

```
- The authoritative publishing gate is server-side in `divine-atbridge` (`crosspost_enabled && ready`, `crates/divine-atbridge/src/runtime.rs:64-76`). Client flags only hide UI and are not a security boundary; widen rollout by enabling opt-in cohorts, not by flipping a client flag.
```

- [ ] **Step 4: Verify no phantom names remain in the checklist**

```bash
grep -n "atprotoPublishing\|enableAtprotoPublishing" docs/runbooks/launch-checklist.md || echo "clean"
```
Expected: `clean`.

- [ ] **Step 5: Verify the real flag name and the gate are now present**

```bash
grep -n "blueskyPublishing\|crosspost_enabled && ready" docs/runbooks/launch-checklist.md
```
Expected: at least the new mobile line and the authoritative-gate line.

---

## Task E3 — Reword the client-flag assertion in `atproto-opt-in-smoke-test.md`

**File (edit, divine-sky):** `docs/runbooks/atproto-opt-in-smoke-test.md` — the `## Failure Checks` block, line 114.

Current text:

```
- Client feature flags must be required to expose the ATProto controls on mobile and web.
```

This is wrong for web (no surface) and vague for mobile (unnamed flag). Replace line 114 with:

- [ ] **Step 1: Replace the line with the verified surfaces + the real gate.**

```
- Mobile exposes the Bluesky crosspost toggle only when `FeatureFlag.blueskyPublishing` (env `FF_BLUESKY_PUBLISHING`, default off) is enabled; web has no ATProto publishing control surface (opt-in is in the keycast console). Either way, publishing is enforced server-side by the bridge's `crosspost_enabled && ready` gate, so a client flag being on must NOT cause publishing for an account that is not `ready` with crosspost enabled.
```

- [ ] **Step 2: Verify**

```bash
grep -n "blueskyPublishing\|crosspost_enabled && ready" docs/runbooks/atproto-opt-in-smoke-test.md
```
Expected: the reworded line.

---

## Task E4 — Cross-reference and close

- [ ] **Step 1: Confirm both runbooks are internally consistent on the gate wording**

```bash
grep -rn "crosspost_enabled && ready" docs/runbooks/launch-checklist.md docs/runbooks/atproto-opt-in-smoke-test.md
```
Expected: matches in both files (launch-checklist already had it at line 80; you added one in E2; E3 added one).

- [ ] **Step 2: Confirm no phantom flag names survive in any runbook**

```bash
grep -rn "atprotoPublishing\|enableAtprotoPublishing" docs/runbooks/ || echo "runbooks clean"
```
Expected: `runbooks clean`.

- [ ] **Step 3: Commit (divine-sky)**

```bash
git add docs/runbooks/launch-checklist.md docs/runbooks/atproto-opt-in-smoke-test.md
git commit -m "docs(rollout): document real crosspost gate, drop phantom client flags"
```
End commit message with the `Co-Authored-By` trailer per repo convention.

- [ ] **Step 4: Mark Chunk E done in the master plan** (`docs/superpowers/plans/2026-05-30-atproto-production-rollout.md`, Task E1 checkboxes) and note in the changelog that the mobile flag exists as `blueskyPublishing`, correcting the "both phantom" claim.

---

## Out of Scope (do not touch)

- `divine-web` and `divine-mobile` source — read-only reference for this chunk. No client code changes (that would be Option B, rejected).
- `docs/superpowers/plans/2026-04-03-phase1-atproto-production-rollout.md` and `docs/superpowers/plans/2026-03-20-divine-atproto-opt-in-provisioning.md` — historical plan files that mention the phantom names. They are not runbooks; leave them as a record of intent. Optionally add a one-line "superseded — see Chunk E" note, but do not rewrite them.

## Self-Review (coverage)

- Decision recorded with rationale (A over B) → Decision section ✓
- Real gate documented (`crosspost_enabled && ready`, file+lines) → Ground Truth #1, E2 Step 3 ✓
- Web phantom confirmed + handled → Ground Truth #2, E2 Step 2 ✓
- Mobile flag real-name correction (`blueskyPublishing`) → Ground Truth #3, E2 Step 1, E3 ✓
- Exact divine-sky runbook files/lines to edit → E2 (launch-checklist.md:39-40), E3 (atproto-opt-in-smoke-test.md:114) ✓
- No sibling-repo edits → Out of Scope ✓

## Risks

- **Trusting the master plan's flag names.** The master plan calls both flags phantom; mobile's is real under another name. Editing on the master plan's wording alone would have deleted a correct (if misnamed) line. Task E1 Step 3 guards against that.
- **Client flag mistaken for a security gate.** If an operator reads "keep the flag off" as the control, a third-party client (or custom build) bypasses it. The E2/E3 wording explicitly subordinates the client flag to the server-side gate.
- **Drift if mobile renames the flag.** If `divine-mobile` later renames `blueskyPublishing`, the runbook line goes stale. Mitigation: the line also names the env var `FF_BLUESKY_PUBLISHING` and points at the server-side gate as authoritative, so a stale display name is non-fatal.
