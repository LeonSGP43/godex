# godex Memory System (QMD Hybrid)

This document is the developer-facing specification for the current `godex`
memory mechanism. It describes runtime flow, retrieval logic, configuration
parameters, and verification requirements.

## Scope

The memory system has two runtime layers:

- Startup write pipeline: Phase 1 extraction + Phase 2 consolidation.
- Read-path recall: semantic shortlist injection into developer instructions.

This design is incremental on Codex's existing memory framework. It does not
introduce external vector databases or external embedding APIs.

## Runtime Architecture

### Entry Conditions

Memory startup is triggered from `start_memories_startup_task` and runs only
when all conditions are met:

- not ephemeral
- `Feature::MemoryTool` enabled
- session is not a sub-agent session
- state DB is available

Code:

- `codex-rs/core/src/memories/start.rs`

### Phase 1 (Per-thread Extraction)

Phase 1 claims stale eligible threads from state DB and extracts stage-1 memory
records in parallel.

Key behavior:

- Startup claim is bounded by config (`max_rollouts_per_startup`).
- Only threads with `memory_mode = 'enabled'` are eligible.
- Fresh/active threads are skipped by idle/age windows.
- Extraction model output schema is:
  - `raw_memory`
  - `rollout_summary`
  - `rollout_slug` (optional)
- Secret redaction is applied before persistence.
- Failures are retried through DB job lease/backoff.

Code:

- `codex-rs/core/src/memories/phase1.rs`
- `codex-rs/state/src/runtime/memories.rs` (`claim_stage1_jobs_for_startup`)

### Phase 2 (Scoped Consolidation)

Phase 2 is serialized per memory scope (single claimed job per selected scope)
and performs:

1. Select stage-1 inputs (usage-aware and freshness-aware)
2. Sync local artifacts under the selected memory root
3. Spawn one internal consolidation sub-agent (no network, no approvals)
4. Commit job watermark and selected snapshot metadata

Key behavior:

- Selection excludes non-enabled memory mode threads.
- Selection ranking prioritizes:
  - `usage_count DESC`
  - `COALESCE(last_usage, source_updated_at) DESC`
  - `source_updated_at DESC`
- `selected_for_phase2` snapshot is tracked to produce `added/retained/removed`.
- The selected scope root is:
  - global scope: `~/.codex/memories` (or `~/.godex/memories` in `-g`)
  - project scope: `~/.codex/memories/scopes/project/<project-scope-dir>`

Code:

- `codex-rs/core/src/memories/phase2.rs`
- `codex-rs/core/src/memories/storage.rs`
- `codex-rs/state/src/runtime/memories.rs` (`get_phase2_input_selection`)

### Read-path Injection

During turn assembly, memory developer instructions are appended only when:

- `Feature::MemoryTool` enabled
- `memories.use_memories = true`
- `memory_summary.md` exists and is non-empty

When semantic helpers are enabled, semantic recall hints are appended as a
shortlist.

Code:

- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/memories/prompts.rs`
- `codex-rs/core/templates/memories/read_path.md`

## QMD Hybrid Retrieval Logic

Current engine metadata:

- `engine = qmd-hybrid-lite`
- `retrieval_pipeline = bm25+vector+rrf+rerank`
- `embedding_backend = local-hash-256`
- vector dimension: `256`

### Index Build

For each retained stage-1 memory item:

- text source = `rollout_summary + raw_memory`
- tokenize to lexical terms
- compute top keywords and top terms
- compute local hash embedding (`local-hash-256`)
- compute BM25 metadata (`idf`, `avg_doc_len`, `k1=1.2`, `b=0.75`)

Artifacts:

- `memory_index.qmd`
- `vector_index.json`

### Query-time Recall

1. Normalize query and tokenize
2. Optional query expansion:
   - non-ASCII tokens get CJK bigrams
3. Compute query embedding (local hash embedding)
4. Candidate filter:
   - vector match when cosine >= `0.03`
   - BM25 match when BM25 > `0`
5. If hybrid enabled:
   - normalize vector and BM25 scores
   - core fusion = `0.6 * vector + 0.4 * bm25`
   - compute RRF (`k = 60`)
   - fused score = `0.7 * core_fusion + 0.3 * rrf`
   - rerank top N (`qmd_rerank_limit`) with:
     - `0.82 * fused + 0.13 * term_overlap + 0.05 * keyword_overlap`
6. Final filter keeps scores >= `0.05`
7. Inject top `semantic_recall_limit` matches into developer instructions

Signals emitted in hints:

- `bm25`
- `vector`
- `rrf`
- `rerank`

Code:

- `codex-rs/core/src/memories/semantic_index.rs`

## Configuration Reference (`[memories]`)

All parameters below are in `config.toml`.

| Key | Default | Range / Clamp | Effect |
| --- | --- | --- | --- |
| `no_memories_if_mcp_or_web_search` | `false` | bool | If `true`, MCP tool calls and web search calls mark thread `memory_mode` as `polluted`, removing it from memory eligibility. |
| `generate_memories` | `true` | bool | If `false`, new threads are created with `memory_mode=disabled`; startup memory extraction for those threads is skipped. |
| `use_memories` | `true` | bool | If `false`, memory developer instructions are not injected into turn context. |
| `scope` | `global` | `global`, `project` | Selects whether memory reads/writes use the shared legacy root or a project-partitioned scope derived from the current project root. |
| `max_raw_memories_for_consolidation` | `256` | `<= 4096` | Upper bound of retained stage-1 memories for consolidation/materialization. |
| `max_unused_days` | `30` | `0..365` | Recency window for retention/pruning and phase-2 input selection. |
| `max_rollout_age_days` | `30` | `0..90` | Max thread age for phase-1 startup claim window. |
| `max_rollouts_per_startup` | `16` | `<= 128` | Max number of rollout jobs claimed per startup run. |
| `min_rollout_idle_hours` | `6` | `1..48` | Minimum idle gap before a thread is eligible for phase-1 extraction. |
| `summary_token_limit` | `5000` | `256..20000` | Max tokens read from `memory_summary.md` when injecting memory guidance into developer instructions. |
| `semantic_index_enabled` | `true` | bool | Enables `memory_index.qmd` / `vector_index.json` generation and semantic recall consumption. |
| `semantic_recall_limit` | `5` | `1..20` | Max number of semantic recall hints appended to developer instructions. |
| `qmd_hybrid_enabled` | `true` | bool | Enables BM25 + vector + RRF + rerank hybrid fusion. |
| `qmd_query_expansion_enabled` | `true` | bool | Enables query expansion (including CJK bigrams) for hybrid recall. |
| `qmd_rerank_limit` | `30` | `1..100` | Max number of top fused candidates reranked in lightweight rerank pass. |
| `extract_model` | `gpt-5.1-codex-mini` | string | Phase-1 extraction model override. |
| `consolidation_model` | `gpt-5.3-codex` | string | Phase-2 consolidation model override. |

Source:

- `codex-rs/core/src/config/types.rs`

## Artifact Contract

Under memory root:

- global scope: `<CODEX_HOME>/memories`
- project scope: `<CODEX_HOME>/memories/scopes/project/<project-scope-dir>`

Within the selected scope root:

- `raw_memories.md`
  - merged stage-1 raw memories (latest first)
- `rollout_summaries/*.md`
  - one per retained rollout summary
- `memory_index.qmd`
  - quick human-readable memory index with keywords and previews
- `vector_index.json`
  - machine-oriented retrieval metadata (vectors + BM25 stats)
- `MEMORY.md`, `memory_summary.md`
  - consolidation outputs used by read-path guidance

## Verification and Test Requirements

When changing this memory system, run at minimum:

1. `cargo check -p codex-core --lib --manifest-path codex-rs/Cargo.toml`
2. `cargo test -p codex-core memories:: -- --nocapture --manifest-path codex-rs/Cargo.toml`
3. `cargo test -p codex-core prompts_tests:: -- --nocapture --manifest-path codex-rs/Cargo.toml`
4. If config types changed:
   - `cargo run -p codex-core --bin codex-write-config-schema --manifest-path codex-rs/Cargo.toml`

Runtime validation (recommended):

1. Start one normal session and produce actions across multiple turns.
2. Restart and verify memory artifacts are refreshed:
   - `memory_index.qmd`
   - `vector_index.json`
   - `raw_memories.md`
3. Confirm semantic hints are present in developer instructions when
   `semantic_index_enabled=true`.
4. Flip `qmd_hybrid_enabled=false` and verify recall behavior falls back to
   vector-only scoring path.

## Troubleshooting

### No memories are generated

Check:

- `Feature::MemoryTool` is enabled.
- `memories.generate_memories = true`.
- session is non-ephemeral and non-sub-agent.
- state DB is available.

### Memories exist but semantic hints are missing

Check:

- `memories.use_memories = true`
- `memories.semantic_index_enabled = true`
- `memory_summary.md` not empty
- `vector_index.json` exists and parses

### Threads disappear from memory candidates

Likely causes:

- thread was marked `polluted` due to MCP/web-search calls when
  `no_memories_if_mcp_or_web_search=true`
- thread was `disabled` at creation (`generate_memories=false`)
- thread is outside `max_unused_days` / age / idle windows
