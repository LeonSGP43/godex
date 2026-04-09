# godex Fork Patch Master Plan

This document is the top-level plan for turning the current `godex` fork into a
maintainable patch-based architecture instead of a deeply coupled source fork.

It supersedes ad-hoc refactor decisions by defining:

- the target fork shape,
- which kinds of customization are acceptable,
- how every fork feature should be packaged,
- how upstream replacement should be detected,
- and how future Codex syncs should be handled without blocking on large
  breakage.

## Core Goal

The goal is **not** to make `godex` magically independent of upstream internals.
That is unrealistic as long as the fork still changes Codex behavior.

The real goal is:

- keep fork behavior alive,
- localize fork ownership,
- make upstream refactors cheaper to absorb,
- make every fork feature independently removable,
- and make upstream replacement decisions explicit.

In other words:

- do **not** aim for "zero sync work forever"
- do aim for "bounded, auditable, patch-group-based sync work"

## Final Target State

Every non-upstream feature should fall into one of only three shapes:

### Shape A: External patch

Best case. The feature lives mostly outside upstream code.

Examples:

- external spawned-agent backends
- maintainer scripts
- release automation
- docs and examples

Allowed upstream touch:

- registration hook
- config loading hook
- tool exposure hook

### Shape B: Hook + adapter patch

Acceptable case. A small number of upstream-hot files call into a fork-owned
adapter or facade.

Examples:

- memory patch facade
- config namespace behavior
- update governance routing

Allowed upstream touch:

- thin hook call
- serialization / config bridge
- patch enable-disable gate

Not allowed:

- large fork logic blocks directly embedded in upstream-hot files

### Shape C: Temporary residue

Transitional only. The feature still overlaps too much with upstream code and
must be reduced later.

Examples today:

- bootstrap login/auth residue
- runtime UI residue
- proxy/MCP residue

Rule:

- no new feature depth should be added here
- only extraction, shrinkage, or deletion is allowed

## Patch Design Rules

Every fork feature must be packaged as a patch group with the following fields:

- `patch_id`
- `purpose`
- `class`
- `owner files`
- `hot overlap files`
- `verification`
- `disable strategy`
- `upstream replacement trigger`
- `deletion condition`

If a customization does not fit this model, it should not be merged.

## Required Architecture Layers

### Layer 1: Upstream-owned base

This is official Codex behavior.

Rule:

- treat upstream as the default owner of product logic
- never expand fork behavior inside upstream files when a hook can be used

### Layer 2: Thin hook layer

This is the only acceptable place where fork behavior may touch upstream-hot
files.

Examples:

- `cli/src/main.rs`
- `core/src/config/mod.rs`
- `core/src/agent/backend.rs`
- `core/src/tools/spec.rs`
- selected memory runtime entrypoints

Rule:

- thin hook files should mostly pass arguments and call fork-owned code
- they should not own fork-specific policy

### Layer 3: Fork-owned patch adapters

This is where fork logic belongs.

Examples:

- `codex-rs/core/src/fork_patch/**`
- `codex-rs/state/src/fork_patch/**`
- external backend worker examples
- fork-specific docs and maintenance scripts

Rule:

- if a feature is fork-owned, its decision logic should live here

### Layer 4: Externalized operator surface

This is the preferred home for anything that does not need to be inside the
binary.

Examples:

- `[agent_backends.*]`
- external backend workers
- maintainer scripts
- release/install wrappers
- docs, manifests, ledgers, runbooks

Rule:

- prefer config, docs, scripts, and external workers over core patches whenever
  behavior can be moved out of process

## Global Completion Criteria

The fork is considered structurally healthy only when all of the following are
true:

1. every non-upstream feature belongs to an explicit patch group
2. every patch group has a disable strategy
3. every patch group has an upstream replacement trigger
4. every patch group has a verification command set
5. upstream-hot files only contain thin hooks, not large fork policy blocks
6. temporary residue is shrinking, not growing
7. future sync review can decide `keep`, `adapt`, or `delete` patch-by-patch

## Current Patch-Group Strategy

### Keep as long-lived durable patch groups

- `fork/provider-backends`
- `fork/config-namespace-home`
- `fork/identity-governance`
- `fork/distribution-release`
- `fork/maintenance-automation`

These define the fork intentionally and should remain explicit.

### Keep behavior, but continue converting into thinner patch-layer form

- `fork/memory-system`

This group is important, but should keep moving away from embedded hot-path
ownership toward `fork_patch` seams.

### Freeze and migrate out

- `fork/native-grok-legacy`

No new capability work should land here. Only migration or removal work is
allowed.

### Shrink aggressively

- `fork/bootstrap-residue`

This is the largest avoidable source of future merge pain and should be treated
as technical debt inventory, not a product surface.

## Master Execution Plan

### Phase 0: Governance freeze

Goal:

- stop mixing product work with fork-shape work

Deliverables:

- keep `docs/godex-fork-manifest.md` as the durable feature manifest
- keep `docs/godex-fork-inventory-ledger.md` as the diff ownership ledger
- keep patch-specific plans such as memory under their own documents
- for any new fork feature, require a manifest entry before implementation

Exit criteria:

- new fork changes are no longer accepted without a named patch group

### Phase 1: Full fork inventory normalization

Goal:

- classify every current modification under one of the approved patch groups

Deliverables:

- no anonymous fork residue
- each owner file mapped to a patch group
- each patch group labeled as `durable`, `migrate`, or `shrink`

Exit criteria:

- sync review can explain every major diff path without rediscovery work

### Phase 2: External backend first

Goal:

- move provider-specific runtime work out of native role/product logic and into
  explicit backend contracts plus external workers

Deliverables:

- `patch/backend-contract` becomes the only supported runtime expansion lane
- provider-specific behavior lives in external workers or backend examples
- built-in roles become backend-oriented metadata, not provider impersonation

Exit criteria:

- adding or removing a provider worker no longer requires deep edits across
  agent runtime internals

Deletion trigger:

- if upstream later ships a stable external backend/plugin seam that is equal or
  better, migrate to it and delete fork-only contract code

### Phase 3: Finish turning memory into a bounded patch group

Goal:

- keep current memory value while preventing future large sync pain

Deliverables:

- retain current MVP closure
- only reopen memory for real blockers, real sync conflicts, or approved later
  refinement
- keep pushing hot-path memory policy into `fork_patch` when justified by real
  maintenance gain

Exit criteria:

- memory sync risk is bounded to known hook files plus `fork_patch` modules

Deletion trigger:

- if upstream lands memory partitioning / recall / runtime features that match
  or beat the fork, delete the overlapping patch subgroup instead of preserving
  it for historical reasons

### Phase 4: Extract config/home/update governance into thin adapters

Goal:

- keep fork identity and namespace behavior while reducing hot-file overlap

Deliverables:

- `cli/src/main.rs` only parses and hands off
- `core/src/config/mod.rs` stops owning fork-specific policy directly
- update governance remains explicit and easy to diff

Exit criteria:

- config and namespace policy are implemented through adapters instead of
  expanding inside upstream config logic

Deletion trigger:

- if upstream exposes equivalent config-namespace and update-routing hooks,
  migrate and delete fork-owned implementation

### Phase 5: Burn down bootstrap residue

Goal:

- shrink non-systematic fork drift that does not deserve long-lived ownership

Targets:

- login/auth residue
- runtime UI residue
- proxy/MCP residue

Allowed work:

- isolate
- delete
- replace with thin adapters

Not allowed:

- adding new feature depth to residue files

Exit criteria:

- bootstrap residue is no longer the largest merge-cost multiplier

### Phase 6: Upstream replacement loop

Goal:

- make patch deletion normal instead of exceptional

For every upstream sync:

1. check which patch groups intersect changed upstream files
2. decide for each group:
   - `keep`
   - `adapt`
   - `delete`
3. prefer upstream-native implementations when they are equal or better
4. keep fork code only where it still materially wins

Exit criteria:

- sync no longer defaults to conflict preservation
- sync becomes a patch-by-patch replacement review

## Required Disable Strategy Per Patch Group

Every patch group should eventually support one of these disable models:

### Mode 1: Config disable

Preferred for runtime features.

Examples:

- disable custom backend worker usage
- disable memory scope override behavior

### Mode 2: Build-time or registration disable

Used when the feature is optional but not runtime-configurable.

Examples:

- remove a built-in compatibility role
- stop registering a fork-specific tool path

### Mode 3: Manifest-level deletion

Used when the patch exists only because upstream does not yet cover it.

Examples:

- delete the patch group when upstream replacement arrives

Rule:

- "we simply keep carrying it forever" is not an acceptable strategy

## Required Upstream Replacement Policy

For every patch group, ask these questions during sync:

1. did upstream add this capability?
2. is upstream behavior equal or better?
3. is migration cost lower than continued fork ownership?
4. does keeping the fork version still produce clear user value?

If `1` and `2` are both yes, default action should be:

- migrate to upstream
- retire the fork patch

## Sync-Safe Operating Rules

To prevent future refactor breakage from becoming catastrophic:

1. never add new fork behavior directly into upstream-hot files without a patch
   group and hook justification
2. never mix product depth with residue cleanup
3. never expand compatibility shims into product platforms
4. always verify by patch group, not only by whole-repo intuition
5. keep manifests and ledgers current before major upstream syncs

## The Practical Promise Of This Plan

This plan does **not** promise:

- zero future merge work
- zero future breakage risk
- immunity from upstream refactors

This plan **does** promise a better operating model:

- upstream refactors should hit bounded surfaces instead of random scattered
  custom code
- each custom feature should be independently explainable, testable, and
  removable
- future syncs should be reviewable as patch groups rather than emergency
  archaeology
- if upstream becomes better, the fork should be able to delete its own code
  cleanly

## Immediate Next Actions

1. treat the current memory work as frozen at the MVP cutline
2. make `patch/backend-contract` the highest-priority structural lane
3. extract `patch/config-home-namespace` into thinner adapters
4. start systematically shrinking `fork/bootstrap-residue`
5. for every later fork feature, require:
   - manifest entry
   - patch group
   - verification
   - disable strategy
   - upstream replacement trigger
