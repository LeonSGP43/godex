use crate::memories::storage::rollout_summary_file_stem;
use chrono::Utc;
use codex_state::Stage1Output;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::io;
use std::path::Path;

const VECTOR_DIMENSION: usize = 256;
const KEYWORD_LIMIT: usize = 12;
const TERM_FREQUENCY_LIMIT: usize = 128;
const SUMMARY_PREVIEW_CHAR_LIMIT: usize = 280;
const MIN_SEMANTIC_RECALL_SCORE: f32 = 0.05;
const MIN_VECTOR_RECALL_SCORE: f32 = 0.03;
const BM25_DEFAULT_K1: f32 = 1.2;
const BM25_DEFAULT_B: f32 = 0.75;
const RRF_K: f32 = 60.0;
const HYBRID_VECTOR_WEIGHT: f32 = 0.6;
const HYBRID_BM25_WEIGHT: f32 = 0.4;
const QMD_ENGINE: &str = "qmd-hybrid-lite";
const QMD_PIPELINE: &str = "bm25+vector+rrf+rerank";
const EMBEDDING_BACKEND: &str = "local-hash-256";

#[derive(Debug, Clone, Copy)]
pub(crate) struct SemanticRecallOptions {
    pub(crate) limit: usize,
    pub(crate) hybrid_enabled: bool,
    pub(crate) query_expansion_enabled: bool,
    pub(crate) rerank_limit: usize,
}

#[derive(Debug, Serialize)]
struct VectorIndex {
    version: u32,
    generated_at: String,
    dimension: usize,
    metric: &'static str,
    engine: &'static str,
    retrieval_pipeline: &'static str,
    embedding_backend: &'static str,
    bm25: Bm25IndexMeta,
    entries: Vec<VectorIndexEntry>,
}

#[derive(Debug, Serialize)]
struct VectorIndexEntry {
    thread_id: String,
    source_updated_at: String,
    rollout_summary_file: String,
    cwd: String,
    git_branch: Option<String>,
    keywords: Vec<String>,
    summary_preview: String,
    top_terms: Vec<TermFrequency>,
    doc_len: usize,
    embedding: Vec<f32>,
}

#[derive(Debug, Serialize)]
struct Bm25IndexMeta {
    k1: f32,
    b: f32,
    avg_doc_len: f32,
    idf: HashMap<String, f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TermFrequency {
    term: String,
    count: u16,
}

#[derive(Debug, Deserialize)]
struct VectorIndexRead {
    dimension: usize,
    #[serde(default)]
    bm25: Option<Bm25IndexRead>,
    entries: Vec<VectorIndexReadEntry>,
}

#[derive(Debug, Deserialize)]
struct VectorIndexReadEntry {
    thread_id: String,
    rollout_summary_file: String,
    keywords: Vec<String>,
    summary_preview: String,
    #[serde(default)]
    top_terms: Vec<TermFrequency>,
    #[serde(default)]
    doc_len: usize,
    embedding: Vec<f32>,
}

#[derive(Debug, Deserialize, Default)]
struct Bm25IndexRead {
    #[serde(default)]
    k1: f32,
    #[serde(default)]
    b: f32,
    #[serde(default)]
    avg_doc_len: f32,
    #[serde(default)]
    idf: HashMap<String, f32>,
}

#[derive(Debug, Clone)]
pub(crate) struct SemanticRecallMatch {
    pub(crate) thread_id: String,
    pub(crate) rollout_summary_file: String,
    pub(crate) keywords: Vec<String>,
    pub(crate) summary_preview: String,
    pub(crate) signals: Vec<String>,
    pub(crate) score: f32,
}

#[derive(Debug)]
struct RankedCandidate {
    entry_index: usize,
    entry: VectorIndexReadEntry,
    vector_score: f32,
    bm25_score: f32,
    rrf_score: f32,
    fused_score: f32,
    final_score: f32,
    matched_vector: bool,
    matched_bm25: bool,
    reranked: bool,
}

pub(super) async fn write_memory_index_qmd(
    root: &Path,
    memories: &[Stage1Output],
) -> io::Result<()> {
    let path = crate::fork_patch::memory::memory_qmd_file(root);
    if memories.is_empty() {
        return tokio::fs::write(path, memory_index_qmd_empty()).await;
    }

    let generated_at = Utc::now().to_rfc3339();
    let mut out = String::new();
    writeln!(out, "---").map_err(format_err)?;
    writeln!(out, "title: Codex Memory QMD Index").map_err(format_err)?;
    writeln!(out, "generated_at: {generated_at}").map_err(format_err)?;
    writeln!(out, "entry_count: {}", memories.len()).map_err(format_err)?;
    writeln!(out, "engine: {QMD_ENGINE}").map_err(format_err)?;
    writeln!(out, "retrieval_pipeline: {QMD_PIPELINE}").map_err(format_err)?;
    writeln!(out, "embedding_backend: {EMBEDDING_BACKEND}").map_err(format_err)?;
    writeln!(
        out,
        "vector_index_file: {}",
        crate::fork_patch::memory::vector_index_file_name()
    )
    .map_err(format_err)?;
    writeln!(out, "---").map_err(format_err)?;
    writeln!(out).map_err(format_err)?;
    writeln!(out, "# Memory Index").map_err(format_err)?;
    writeln!(out).map_err(format_err)?;
    writeln!(
        out,
        "This file is generated from stage-1 outputs (latest-first ordering)."
    )
    .map_err(format_err)?;
    writeln!(out).map_err(format_err)?;

    for (position, memory) in memories.iter().enumerate() {
        let rollout_summary_file = crate::fork_patch::memory::rollout_summary_relative_path(
            &rollout_summary_file_stem(memory),
        );
        let combined_text = format!("{}\n{}", memory.rollout_summary, memory.raw_memory);
        let keywords = top_keywords(&combined_text, KEYWORD_LIMIT).join(", ");
        let summary_preview =
            truncate_chars(memory.rollout_summary.trim(), SUMMARY_PREVIEW_CHAR_LIMIT);

        writeln!(out, "## Entry {}", position + 1).map_err(format_err)?;
        writeln!(out, "- thread_id: {}", memory.thread_id).map_err(format_err)?;
        writeln!(
            out,
            "- updated_at: {}",
            memory.source_updated_at.to_rfc3339()
        )
        .map_err(format_err)?;
        writeln!(out, "- rollout_summary_file: {rollout_summary_file}").map_err(format_err)?;
        writeln!(out, "- cwd: {}", memory.cwd.display()).map_err(format_err)?;
        if let Some(branch) = memory.git_branch.as_deref() {
            writeln!(out, "- git_branch: {branch}").map_err(format_err)?;
        }
        if !keywords.is_empty() {
            writeln!(out, "- keywords: {keywords}").map_err(format_err)?;
        }
        writeln!(out).map_err(format_err)?;
        writeln!(out, "### Summary").map_err(format_err)?;
        if summary_preview.is_empty() {
            writeln!(out, "- (empty rollout summary)").map_err(format_err)?;
        } else {
            writeln!(out, "{summary_preview}").map_err(format_err)?;
        }
        writeln!(out).map_err(format_err)?;
    }

    tokio::fs::write(path, out).await
}

pub(super) async fn write_vector_index_json(
    root: &Path,
    memories: &[Stage1Output],
) -> io::Result<()> {
    let mut doc_frequency = HashMap::<String, usize>::new();
    let entries = memories
        .iter()
        .map(|memory| {
            let combined_text = format!("{}\n{}", memory.rollout_summary, memory.raw_memory);
            let rollout_summary_file = crate::fork_patch::memory::rollout_summary_relative_path(
                &rollout_summary_file_stem(memory),
            );
            let lexical_counts = lexical_term_counts(&combined_text);
            for term in lexical_counts.keys() {
                *doc_frequency.entry(term.clone()).or_default() += 1;
            }
            let doc_len = lexical_counts.values().sum::<usize>();
            VectorIndexEntry {
                thread_id: memory.thread_id.to_string(),
                source_updated_at: memory.source_updated_at.to_rfc3339(),
                rollout_summary_file,
                cwd: memory.cwd.display().to_string(),
                git_branch: memory.git_branch.clone(),
                keywords: top_keywords(&combined_text, KEYWORD_LIMIT),
                summary_preview: truncate_chars(
                    memory.rollout_summary.trim(),
                    SUMMARY_PREVIEW_CHAR_LIMIT,
                ),
                top_terms: top_terms_from_counts(&lexical_counts, TERM_FREQUENCY_LIMIT),
                doc_len,
                embedding: embedding_for_text(&combined_text),
            }
        })
        .collect::<Vec<_>>();
    let avg_doc_len = if entries.is_empty() {
        0.0
    } else {
        entries
            .iter()
            .map(|entry| entry.doc_len as f32)
            .sum::<f32>()
            / entries.len() as f32
    };
    let idf = doc_frequency
        .into_iter()
        .filter_map(|(term, df)| {
            let idf = bm25_idf(entries.len(), df);
            (idf > 0.0).then_some((term, idf))
        })
        .collect::<HashMap<_, _>>();

    let index = VectorIndex {
        version: 2,
        generated_at: Utc::now().to_rfc3339(),
        dimension: VECTOR_DIMENSION,
        metric: "cosine",
        engine: QMD_ENGINE,
        retrieval_pipeline: QMD_PIPELINE,
        embedding_backend: EMBEDDING_BACKEND,
        bm25: Bm25IndexMeta {
            k1: BM25_DEFAULT_K1,
            b: BM25_DEFAULT_B,
            avg_doc_len,
            idf,
        },
        entries,
    };
    let body = serde_json::to_string_pretty(&index)
        .map_err(|err| io::Error::other(format!("serialize vector index: {err}")))?;
    tokio::fs::write(crate::fork_patch::memory::vector_index_file(root), body).await
}

pub(super) async fn clear_auxiliary_indexes(root: &Path) -> io::Result<()> {
    for path in [
        crate::fork_patch::memory::memory_qmd_file(root),
        crate::fork_patch::memory::vector_index_file(root),
    ] {
        if let Err(err) = tokio::fs::remove_file(path).await
            && err.kind() != io::ErrorKind::NotFound
        {
            return Err(err);
        }
    }
    Ok(())
}

pub(crate) async fn semantic_recall(
    root: &Path,
    query: &str,
    options: SemanticRecallOptions,
) -> io::Result<Vec<SemanticRecallMatch>> {
    if query.trim().is_empty() || options.limit == 0 {
        return Ok(Vec::new());
    }

    let body =
        match tokio::fs::read_to_string(crate::fork_patch::memory::vector_index_file(root)).await {
            Ok(body) => body,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err),
        };
    let index: VectorIndexRead = serde_json::from_str(&body)
        .map_err(|err| io::Error::other(format!("parse vector index: {err}")))?;
    if index.dimension == 0 {
        return Ok(Vec::new());
    }

    let query_terms = build_query_terms(query, options.query_expansion_enabled);
    let query_embedding_input = if !options.query_expansion_enabled || query_terms.is_empty() {
        query.to_string()
    } else {
        format!("{query}\n{}", query_terms.join(" "))
    };
    let query_embedding = embedding_for_text(&query_embedding_input);
    if query_terms.is_empty() && query_embedding.iter().all(|value| *value == 0.0_f32) {
        return Ok(Vec::new());
    }
    let bm25 = index.bm25.unwrap_or_default();
    let mut candidates = index
        .entries
        .into_iter()
        .enumerate()
        .filter_map(|(entry_index, entry)| {
            if entry.embedding.len() != index.dimension {
                return None;
            }
            let vector_score = cosine_similarity(&query_embedding, &entry.embedding);
            let bm25_score = if options.hybrid_enabled {
                bm25_score_for_entry(&query_terms, &entry, &bm25)
            } else {
                0.0
            };
            let matched_vector =
                vector_score.is_finite() && vector_score >= MIN_VECTOR_RECALL_SCORE;
            let matched_bm25 = bm25_score.is_finite() && bm25_score > 0.0;
            if !(matched_vector || matched_bm25) {
                return None;
            }
            Some(RankedCandidate {
                entry_index,
                entry,
                vector_score,
                bm25_score,
                rrf_score: 0.0,
                fused_score: 0.0,
                final_score: 0.0,
                matched_vector,
                matched_bm25,
                reranked: false,
            })
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    if options.hybrid_enabled {
        apply_hybrid_scores(&mut candidates);
        let rerank_limit = options.rerank_limit.max(1).min(candidates.len());
        apply_lightweight_rerank(&mut candidates, &query_terms, rerank_limit);
    } else {
        for candidate in &mut candidates {
            candidate.fused_score = candidate.vector_score.max(0.0);
            candidate.final_score = candidate.fused_score;
        }
    }

    candidates.sort_by(|left, right| right.final_score.total_cmp(&left.final_score));
    let matches = candidates
        .into_iter()
        .filter_map(|candidate| {
            if !candidate.final_score.is_finite()
                || candidate.final_score < MIN_SEMANTIC_RECALL_SCORE
            {
                return None;
            }
            let mut signals = Vec::with_capacity(4);
            if candidate.matched_bm25 {
                signals.push("bm25".to_string());
            }
            if candidate.matched_vector {
                signals.push("vector".to_string());
            }
            if options.hybrid_enabled && candidate.rrf_score > 0.0 {
                signals.push("rrf".to_string());
            }
            if candidate.reranked {
                signals.push("rerank".to_string());
            }
            Some(SemanticRecallMatch {
                thread_id: candidate.entry.thread_id,
                rollout_summary_file: candidate.entry.rollout_summary_file,
                keywords: candidate.entry.keywords,
                summary_preview: candidate.entry.summary_preview,
                signals,
                score: candidate.final_score,
            })
        })
        .take(options.limit)
        .collect::<Vec<_>>();
    Ok(matches)
}

fn apply_hybrid_scores(candidates: &mut [RankedCandidate]) {
    let vector_rank = build_rank_map(
        candidates,
        |candidate| candidate.matched_vector,
        |candidate| candidate.vector_score,
    );
    let bm25_rank = build_rank_map(
        candidates,
        |candidate| candidate.matched_bm25,
        |candidate| candidate.bm25_score,
    );
    for candidate in &mut *candidates {
        candidate.rrf_score = rrf_score(candidate.entry_index, &vector_rank, &bm25_rank);
    }

    let vector_bounds = score_bounds(candidates.iter().map(|candidate| candidate.vector_score));
    let bm25_bounds = score_bounds(candidates.iter().map(|candidate| candidate.bm25_score));
    let rrf_bounds = score_bounds(candidates.iter().map(|candidate| candidate.rrf_score));

    for candidate in &mut *candidates {
        let vector_norm = normalize_score(candidate.vector_score, vector_bounds);
        let bm25_norm = normalize_score(candidate.bm25_score, bm25_bounds);
        let fusion_core = HYBRID_VECTOR_WEIGHT * vector_norm + HYBRID_BM25_WEIGHT * bm25_norm;
        let rrf_norm = normalize_score(candidate.rrf_score, rrf_bounds);
        candidate.fused_score = (fusion_core * 0.7) + (rrf_norm * 0.3);
        candidate.final_score = candidate.fused_score;
    }
}

fn apply_lightweight_rerank(
    candidates: &mut [RankedCandidate],
    query_terms: &[String],
    rerank_limit: usize,
) {
    candidates.sort_by(|left, right| right.fused_score.total_cmp(&left.fused_score));

    for (idx, candidate) in candidates.iter_mut().enumerate() {
        if idx >= rerank_limit {
            candidate.final_score = candidate.fused_score;
            continue;
        }
        let term_overlap = overlap_ratio(
            query_terms,
            candidate
                .entry
                .top_terms
                .iter()
                .map(|term| term.term.as_str()),
        );
        let keyword_terms = candidate
            .entry
            .keywords
            .iter()
            .flat_map(|keyword| tokenize(keyword))
            .collect::<HashSet<_>>();
        let keyword_overlap = overlap_ratio_for_owned(query_terms, &keyword_terms);
        candidate.final_score =
            (candidate.fused_score * 0.82) + (term_overlap * 0.13) + (keyword_overlap * 0.05);
        candidate.reranked = true;
    }
}

fn build_rank_map<FInclude, FScore>(
    candidates: &[RankedCandidate],
    include: FInclude,
    score: FScore,
) -> HashMap<usize, usize>
where
    FInclude: Fn(&RankedCandidate) -> bool,
    FScore: Fn(&RankedCandidate) -> f32,
{
    let mut ranked = candidates
        .iter()
        .filter(|candidate| include(candidate))
        .map(|candidate| (candidate.entry_index, score(candidate)))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.total_cmp(&left.1));
    ranked
        .into_iter()
        .enumerate()
        .map(|(idx, (entry_index, _))| (entry_index, idx + 1))
        .collect()
}

fn rrf_score(
    entry_index: usize,
    vector_rank: &HashMap<usize, usize>,
    bm25_rank: &HashMap<usize, usize>,
) -> f32 {
    let mut score = 0.0;
    if let Some(rank) = vector_rank.get(&entry_index) {
        score += 1.0 / (RRF_K + *rank as f32);
    }
    if let Some(rank) = bm25_rank.get(&entry_index) {
        score += 1.0 / (RRF_K + *rank as f32);
    }
    score
}

fn score_bounds(scores: impl Iterator<Item = f32>) -> Option<(f32, f32)> {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for score in scores {
        if !score.is_finite() {
            continue;
        }
        min = min.min(score);
        max = max.max(score);
    }
    if !min.is_finite() || !max.is_finite() {
        None
    } else {
        Some((min, max))
    }
}

fn normalize_score(score: f32, bounds: Option<(f32, f32)>) -> f32 {
    if !score.is_finite() {
        return 0.0;
    }
    match bounds {
        None => 0.0,
        Some((min, max)) if (max - min).abs() < f32::EPSILON => (score > 0.0) as u8 as f32,
        Some((min, max)) => ((score - min) / (max - min)).clamp(0.0, 1.0),
    }
}

fn overlap_ratio<'a>(query_terms: &[String], target_terms: impl Iterator<Item = &'a str>) -> f32 {
    if query_terms.is_empty() {
        return 0.0;
    }
    let target_terms = target_terms.collect::<HashSet<_>>();
    if target_terms.is_empty() {
        return 0.0;
    }
    let overlap = query_terms
        .iter()
        .filter(|term| target_terms.contains(term.as_str()))
        .count();
    overlap as f32 / query_terms.len() as f32
}

fn overlap_ratio_for_owned(query_terms: &[String], target_terms: &HashSet<String>) -> f32 {
    if query_terms.is_empty() || target_terms.is_empty() {
        return 0.0;
    }
    let overlap = query_terms
        .iter()
        .filter(|term| target_terms.contains(term.as_str()))
        .count();
    overlap as f32 / query_terms.len() as f32
}

fn build_query_terms(query: &str, query_expansion_enabled: bool) -> Vec<String> {
    let mut terms = tokenize(query)
        .into_iter()
        .filter(|token| token.len() >= 2 && !is_stopword(token))
        .collect::<Vec<_>>();
    if query_expansion_enabled {
        let base_terms = terms.clone();
        for term in &base_terms {
            if !term.is_ascii() {
                terms.extend(cjk_bigrams(term));
            }
        }
    }
    let mut seen = HashSet::<String>::new();
    terms.retain(|token| !token.is_empty() && seen.insert(token.clone()));
    terms
}

fn bm25_score_for_entry(
    query_terms: &[String],
    entry: &VectorIndexReadEntry,
    bm25: &Bm25IndexRead,
) -> f32 {
    if query_terms.is_empty() || entry.top_terms.is_empty() {
        return 0.0;
    }
    let k1 = if bm25.k1 > 0.0 {
        bm25.k1
    } else {
        BM25_DEFAULT_K1
    };
    let b = if bm25.b > 0.0 {
        bm25.b.clamp(0.0, 1.0)
    } else {
        BM25_DEFAULT_B
    };
    let avg_doc_len = if bm25.avg_doc_len > 0.0 {
        bm25.avg_doc_len
    } else {
        entry.doc_len.max(1) as f32
    };
    let doc_len = entry.doc_len.max(1) as f32;
    let norm = k1 * (1.0 - b + b * (doc_len / avg_doc_len.max(1.0)));

    query_terms.iter().fold(0.0, |acc, term| {
        let tf = entry
            .top_terms
            .iter()
            .find(|top_term| top_term.term == *term)
            .map(|top_term| f32::from(top_term.count))
            .unwrap_or(0.0);
        if tf <= 0.0 {
            return acc;
        }
        let idf = bm25.idf.get(term).copied().unwrap_or(0.0);
        if idf <= 0.0 {
            return acc;
        }
        acc + idf * ((tf * (k1 + 1.0)) / (tf + norm))
    })
}

fn bm25_idf(total_docs: usize, doc_freq: usize) -> f32 {
    if total_docs == 0 || doc_freq == 0 {
        return 0.0;
    }
    let n = total_docs as f32;
    let df = doc_freq as f32;
    (((n - df + 0.5) / (df + 0.5)) + 1.0).ln().max(0.0)
}

fn memory_index_qmd_empty() -> String {
    let generated_at = Utc::now().to_rfc3339();
    format!(
        "---\ntitle: Codex Memory QMD Index\ngenerated_at: {generated_at}\nentry_count: 0\nengine: {QMD_ENGINE}\nretrieval_pipeline: {QMD_PIPELINE}\nembedding_backend: {EMBEDDING_BACKEND}\nvector_index_file: {}\n---\n\n# Memory Index\n\nNo memory entries yet.\n",
        crate::fork_patch::memory::vector_index_file_name()
    )
}

fn embedding_for_text(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0_f32; VECTOR_DIMENSION];
    for token in tokenize(text) {
        if token.len() < 2 {
            continue;
        }
        let idx = (stable_hash(&token) as usize) % VECTOR_DIMENSION;
        vector[idx] += 1.0;
    }

    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }
    vector
}

fn top_keywords(text: &str, max_keywords: usize) -> Vec<String> {
    let counts = lexical_term_counts(text);
    let mut scored = counts
        .into_iter()
        .filter(|(token, _)| token.len() >= 3)
        .collect::<Vec<_>>();
    scored.sort_by(|(left_token, left_count), (right_token, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_token.cmp(right_token))
    });
    scored
        .into_iter()
        .take(max_keywords)
        .map(|(token, _)| token)
        .collect()
}

fn top_terms_from_counts(counts: &HashMap<String, usize>, max_terms: usize) -> Vec<TermFrequency> {
    let mut scored = counts
        .iter()
        .map(|(token, count)| (token.clone(), *count))
        .collect::<Vec<_>>();
    scored.sort_by(|(left_token, left_count), (right_token, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_token.cmp(right_token))
    });
    scored
        .into_iter()
        .take(max_terms)
        .map(|(term, count)| TermFrequency {
            term,
            count: count.min(u16::MAX as usize) as u16,
        })
        .collect()
}

fn lexical_term_counts(text: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::<String, usize>::new();
    for token in tokenize(text) {
        if token.len() < 2 || is_stopword(&token) {
            continue;
        }
        *counts.entry(token.clone()).or_default() += 1;
        if !token.is_ascii() {
            for gram in cjk_bigrams(&token) {
                *counts.entry(gram).or_default() += 1;
            }
        }
    }
    counts
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            for lower in ch.to_lowercase() {
                current.push(lower);
            }
            continue;
        }
        if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn cjk_bigrams(token: &str) -> Vec<String> {
    let chars = token.chars().collect::<Vec<_>>();
    if chars.len() < 2 {
        return Vec::new();
    }
    chars
        .windows(2)
        .map(|pair| pair.iter().collect::<String>())
        .collect()
}

fn is_stopword(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "and"
            | "for"
            | "with"
            | "that"
            | "this"
            | "from"
            | "into"
            | "not"
            | "but"
            | "you"
            | "your"
            | "use"
            | "using"
            | "can"
            | "may"
            | "should"
            | "must"
            | "true"
            | "false"
    )
}

fn stable_hash(token: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in token.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3_u64);
    }
    hash
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let truncated = value.chars().take(max_chars).collect::<String>();
    format!("{truncated}...")
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    let dot = left
        .iter()
        .zip(right.iter())
        .map(|(l, r)| l * r)
        .sum::<f32>();
    let left_norm = left.iter().map(|value| value * value).sum::<f32>().sqrt();
    let right_norm = right.iter().map(|value| value * value).sum::<f32>().sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }
    dot / (left_norm * right_norm)
}

fn format_err(err: std::fmt::Error) -> io::Error {
    io::Error::other(format!("format memory index: {err}"))
}
