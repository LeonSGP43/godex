# godex Development Plan

## Target State

Build this fork as `godex`, a product that can run in parallel with official
Codex while supporting two config namespaces:

- default mode: fully reuse Codex config locations and loading behavior
  - global: `~/.codex`
  - project: `.codex`
- isolated mode: enabled explicitly with `godex -g`
  - global: `~/.godex`
  - project: `.godex`

Both modes should preserve the same config semantics, precedence rules, and
project trust behavior. The only difference should be which config namespace is
used.

## Goals

1. Rename all user-facing `godex` branding to `godex`.
2. Make `godex` default to Codex-compatible config loading.
3. Add `-g` to switch all global/project config resolution to the `godex`
   namespace.
4. Introduce first-class `godex` version management and release detection.
5. Keep upstream Codex comparison logic, but demote it to a secondary startup
   notice showing release gap.
6. Keep upstream sync flow easy for a long-lived fork.

## Non-Goals

- Renaming internal workspace crate ids from `codex-*` to `godex-*`.
- Rewriting the full config schema or project trust model.
- Replacing the existing upstream sync mechanism with a new git workflow.

## Constraints

- Preserve existing user changes in this worktree.
- Keep fork-specific logic localized to a small set of modules.
- Prefer additive changes over invasive rewrites.
- Maintain compatibility for existing Codex-style config files by default.

## Architecture Direction

### 1. Config Namespace

Introduce a small runtime concept, `ConfigNamespace`, with two values:

- `CodexCompatible`
- `GodexIsolated`

This namespace must drive:

- global config home resolution
- project-layer directory scanning (`.codex` vs `.godex`)
- config writes (`config.toml`, features, MCP edits, trust state)
- home-relative assets such as logs, themes, plugins, memories, prompts

Default behavior for `godex` should be `CodexCompatible`.
Passing `-g` should switch to `GodexIsolated`.

### 2. Branding

Standardize user-visible naming to:

- app display name: `godex`
- executable name: `godex`
- package-facing version label: `godex`

Keep internal crate names unchanged unless a technical need appears.

### 3. Versioning

Add first-class fork version governance:

- root `VERSION`
- root `CHANGELOG.md`
- workspace/package version wiring so `godex --version` is meaningful
- tests or checks that keep version metadata aligned

`godex` update checks should compare against the latest release in the fork's
own GitHub repository, configurable via repo slug.

### 4. Update UX

Split startup version information into two concerns:

- `godex` update prompt:
  - "your installed godex is behind the latest godex release"
- upstream Codex gap notice:
  - "official Codex is N releases ahead"

The upstream comparison should use upstream release history and the local
upstream baseline tag from the source checkout.

### 5. Upstream Sync Workflow

Retain `sync-upstream`, but make it fork-oriented:

- fetch configured upstream remote
- merge or fast-forward from configured branch
- rebuild `godex`
- report current fork version and upstream release gap

Long term, fork-specific code should be concentrated in:

- branding
- config namespace selection
- godex release/update logic
- upstream sync helpers

## Execution Phases

### Phase 0. Stabilize Current Branch

- Keep the current in-progress fork/update changes as the base.
- Rename all `godex` branding to `godex`.
- Preserve passing targeted tests before deeper refactors.

Acceptance:

- `godex --version` prints `godex ...`
- no user-facing `godex` strings remain in touched surfaces

### Phase 1. Introduce Config Namespace Plumbing

- Add `ConfigNamespace` and a namespace-aware home resolver.
- Thread namespace selection through config load/build paths.
- Add namespace-aware project layer scanning (`.codex` or `.godex`).
- Make default `godex` mode use Codex-compatible namespace.
- Add top-level `-g` flag to switch namespace.

Acceptance:

- `godex` loads `~/.codex` and project `.codex`
- `godex -g` loads `~/.godex` and project `.godex`
- feature toggles / config writes target the selected namespace

### Phase 2. Rename Fork-Specific Docs and UX

- Update docs, prompts, status cards, history headers, and help text to
  consistently say `godex`.
- Ensure startup/update copy differentiates `godex` from official Codex.

Acceptance:

- startup UI shows `godex`
- update messaging never says `godex`
- docs explain default mode vs `-g` mode

### Phase 3. Add godex Version Governance

- Introduce `VERSION` and `CHANGELOG.md` at repo root if absent.
- Wire workspace/package version to the managed version source.
- Add tests/checks for version metadata alignment.

Acceptance:

- `godex --version` reports managed fork version
- version metadata is auditable and release-ready

### Phase 4. Split Update Logic

- Add `[godex_updates]` config for the fork's own release detection.
- Keep `[upstream_updates]` for source sync and official Codex comparison.
- Implement two startup notices:
  - godex release available
  - official Codex release gap

Acceptance:

- `godex` can tell when fork release is outdated
- `godex` can show official Codex release gap count
- both signals are independently configurable

### Phase 5. Tighten Upstream Maintenance Flow

- Refine `godex sync-upstream` output and controls.
- Add release-gap reporting after sync.
- Document a recommended fork maintenance workflow.

Acceptance:

- sync command is usable for day-to-day upstream maintenance
- docs explain how to update from upstream and cut godex releases

## Testing Strategy

Run tests in increasing scope after each phase:

- targeted unit tests for config path resolution
- targeted CLI tests for `godex` branding and `-g`
- targeted TUI/app-server snapshot tests for branding/update text
- targeted config schema test when config fields change
- final build of `godex` binary and direct CLI smoke checks

## Risks

### Risk: namespace changes break project trust/config precedence

Mitigation:

- keep namespace as a narrow input into existing loader logic
- add tests for both `.codex` and `.godex` project-layer discovery

### Risk: version logic becomes fork-specific in too many places

Mitigation:

- centralize fork metadata in one branding/version module

### Risk: upstream merges stay expensive

Mitigation:

- keep fork-only code isolated to a few files
- avoid renaming internal crates

## Immediate Next Step

Implement Phase 0 and Phase 1 foundations:

1. rename visible `godex` branding to `godex`
2. add a namespace abstraction for config home/project layer selection
3. wire default mode to `.codex` and `-g` mode to `.godex`
