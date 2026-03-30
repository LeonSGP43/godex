use crate::memories::memory_qmd_file;
use crate::memories::storage::rollout_summary_file_stem;
use crate::memories::vector_index_file;
use chrono::Utc;
use codex_state::Stage1Output;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io;
use std::path::Path;

const VECTOR_DIMENSION: usize = 256;
const KEYWORD_LIMIT: usize = 12;
const SUMMARY_PREVIEW_CHAR_LIMIT: usize = 280;
const MIN_SEMANTIC_RECALL_SCORE: f32 = 0.05;

#[derive(Debug, Serialize)]
struct VectorIndex {
    version: u32,
    generated_at: String,
    dimension: usize,
    metric: &'static str,
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
    embedding: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct VectorIndexRead {
    dimension: usize,
    entries: Vec<VectorIndexReadEntry>,
}

#[derive(Debug, Deserialize)]
struct VectorIndexReadEntry {
    thread_id: String,
    rollout_summary_file: String,
    keywords: Vec<String>,
    summary_preview: String,
    embedding: Vec<f32>,
}

#[derive(Debug, Clone)]
pub(super) struct SemanticRecallMatch {
    pub(super) thread_id: String,
    pub(super) rollout_summary_file: String,
    pub(super) keywords: Vec<String>,
    pub(super) summary_preview: String,
    pub(super) score: f32,
}

pub(super) async fn write_memory_index_qmd(
    root: &Path,
    memories: &[Stage1Output],
) -> io::Result<()> {
    let path = memory_qmd_file(root);
    if memories.is_empty() {
        return tokio::fs::write(path, memory_index_qmd_empty()).await;
    }

    let generated_at = Utc::now().to_rfc3339();
    let mut out = String::new();
    writeln!(out, "---").map_err(format_err)?;
    writeln!(out, "title: Codex Memory QMD Index").map_err(format_err)?;
    writeln!(out, "generated_at: {generated_at}").map_err(format_err)?;
    writeln!(out, "entry_count: {}", memories.len()).map_err(format_err)?;
    writeln!(out, "vector_index_file: vector_index.json").map_err(format_err)?;
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
        let rollout_summary_file =
            format!("rollout_summaries/{}.md", rollout_summary_file_stem(memory));
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
    let entries = memories
        .iter()
        .map(|memory| {
            let combined_text = format!("{}\n{}", memory.rollout_summary, memory.raw_memory);
            let rollout_summary_file =
                format!("rollout_summaries/{}.md", rollout_summary_file_stem(memory));
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
                embedding: embedding_for_text(&combined_text),
            }
        })
        .collect::<Vec<_>>();

    let index = VectorIndex {
        version: 1,
        generated_at: Utc::now().to_rfc3339(),
        dimension: VECTOR_DIMENSION,
        metric: "cosine",
        entries,
    };
    let body = serde_json::to_string_pretty(&index)
        .map_err(|err| io::Error::other(format!("serialize vector index: {err}")))?;
    tokio::fs::write(vector_index_file(root), body).await
}

pub(super) async fn clear_auxiliary_indexes(root: &Path) -> io::Result<()> {
    for path in [memory_qmd_file(root), vector_index_file(root)] {
        if let Err(err) = tokio::fs::remove_file(path).await
            && err.kind() != io::ErrorKind::NotFound
        {
            return Err(err);
        }
    }
    Ok(())
}

pub(super) async fn semantic_recall(
    root: &Path,
    query: &str,
    limit: usize,
) -> io::Result<Vec<SemanticRecallMatch>> {
    if query.trim().is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let body = match tokio::fs::read_to_string(vector_index_file(root)).await {
        Ok(body) => body,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let index: VectorIndexRead = serde_json::from_str(&body)
        .map_err(|err| io::Error::other(format!("parse vector index: {err}")))?;
    if index.dimension == 0 {
        return Ok(Vec::new());
    }

    let query_embedding = embedding_for_text(query);
    if query_embedding.iter().all(|value| *value == 0.0_f32) {
        return Ok(Vec::new());
    }

    let mut matches = index
        .entries
        .into_iter()
        .filter_map(|entry| {
            if entry.embedding.len() != index.dimension {
                return None;
            }
            let score = cosine_similarity(&query_embedding, &entry.embedding);
            if !score.is_finite() || score < MIN_SEMANTIC_RECALL_SCORE {
                return None;
            }
            Some(SemanticRecallMatch {
                thread_id: entry.thread_id,
                rollout_summary_file: entry.rollout_summary_file,
                keywords: entry.keywords,
                summary_preview: entry.summary_preview,
                score,
            })
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| right.score.total_cmp(&left.score));
    matches.truncate(limit);
    Ok(matches)
}

fn memory_index_qmd_empty() -> String {
    let generated_at = Utc::now().to_rfc3339();
    format!(
        "---\ntitle: Codex Memory QMD Index\ngenerated_at: {generated_at}\nentry_count: 0\nvector_index_file: vector_index.json\n---\n\n# Memory Index\n\nNo memory entries yet.\n"
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
    let mut counts = HashMap::<String, usize>::new();
    for token in tokenize(text) {
        if token.len() < 3 || is_stopword(&token) {
            continue;
        }
        *counts.entry(token).or_default() += 1;
    }

    let mut scored = counts.into_iter().collect::<Vec<_>>();
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
            | "when"
            | "where"
            | "while"
            | "then"
            | "than"
            | "are"
            | "was"
            | "were"
            | "have"
            | "has"
            | "had"
            | "not"
            | "but"
            | "you"
            | "your"
            | "they"
            | "them"
            | "its"
            | "our"
            | "all"
            | "use"
            | "using"
            | "used"
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
