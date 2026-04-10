use regex_lite::Regex;
use reqwest::header::ACCEPT;
use reqwest::header::AUTHORIZATION;
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use std::env;
use std::sync::OnceLock;
use uuid::Uuid;

use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use codex_config::types::GrokConfig as RuntimeGrokConfig;
use codex_config::types::GrokPresetId;
use codex_config::types::GrokPresetsConfig;
use codex_login::default_client::build_reqwest_client;

pub(crate) const GROK_TOOL_NAME: &str = "grok";

const DEFAULT_GROK_APP_NAME: &str = "codex-grok";
const DEFAULT_SOURCE_NOTE_LIMIT: usize = 5;
const MAX_SOURCE_NOTE_LIMIT: usize = 10;
const GROK_SYSTEM_PROMPT: &str = r#"You are a Grok-backed research worker.

Answer in concise Markdown. Use the selected preset to determine style:
- b42: latest information, source-oriented search, fact finding
- thinking41: research planning, decomposition, investigation strategy
- expert41: judgment, synthesis, and polished reports from known material

Include direct source URLs when possible and avoid inventing facts."#;

pub(crate) struct GrokHandler;

#[derive(Debug, Deserialize)]
struct GrokArgs {
    query: String,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default = "default_source_note_limit")]
    source_note_limit: usize,
}

#[derive(Clone)]
struct GrokToolConfig {
    base_origin: String,
    api_key: String,
    default_dynamic_model: String,
    default_preset: GrokPresetId,
    presets: GrokPresetsConfig,
}

#[derive(Clone)]
struct GrokRequestConfig {
    preset: GrokPresetId,
    endpoint: String,
    api_key: String,
    model: String,
}

#[derive(Clone, Debug)]
struct SourceNote {
    url: String,
}

impl ToolHandler for GrokHandler {
    type Output = FunctionToolOutput;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        let config = GrokToolConfig::from_invocation(&invocation)?;
        let arguments = function_arguments(invocation.payload)?;
        let args: GrokArgs = parse_arguments(&arguments)?;
        let source_note_limit = clamp_source_note_limit(args.source_note_limit);
        let platform = normalized_optional_text(args.platform);
        let preset =
            parse_preset_override(args.preset.as_deref())?.unwrap_or(config.default_preset);
        let request = config.request_for_preset(preset, args.model.as_deref());
        let answer = query_grok(&request, &args.query, platform.as_deref()).await?;
        let notes = extract_unique_urls(&answer)
            .into_iter()
            .take(source_note_limit)
            .map(|url| SourceNote { url })
            .collect::<Vec<_>>();
        let output = render_output(
            Uuid::new_v4().simple().to_string(),
            &request,
            &args.query,
            &answer,
            &notes,
        );
        Ok(FunctionToolOutput::from_text(output, Some(true)))
    }
}

fn function_arguments(payload: ToolPayload) -> Result<String, FunctionCallError> {
    match payload {
        ToolPayload::Function { arguments } => Ok(arguments),
        _ => Err(FunctionCallError::RespondToModel(
            "grok handler received unsupported payload".to_string(),
        )),
    }
}

impl GrokToolConfig {
    fn from_invocation(invocation: &ToolInvocation) -> Result<Self, FunctionCallError> {
        let runtime_config: &RuntimeGrokConfig = &invocation.turn.config.grok;
        let api_key = env::var(&runtime_config.api_key_env)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                env::var("GROK_API_KEY")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
            .or_else(|| {
                env::var("XAI_API_KEY")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
            .or_else(|| {
                env::var("OPENAI_API_KEY")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
            .ok_or_else(|| {
                FunctionCallError::RespondToModel(format!(
                    "missing Grok credentials; set {}, GROK_API_KEY, XAI_API_KEY, or OPENAI_API_KEY",
                    runtime_config.api_key_env
                ))
            })?;

        Ok(Self {
            base_origin: runtime_config.base_origin.trim_end_matches('/').to_string(),
            api_key,
            default_dynamic_model: runtime_config.default_dynamic_model.clone(),
            default_preset: runtime_config.default_preset,
            presets: runtime_config.presets.clone(),
        })
    }

    fn request_for_preset(
        &self,
        preset: GrokPresetId,
        model_override: Option<&str>,
    ) -> GrokRequestConfig {
        let preset_config = self.presets.get(preset);
        let endpoint = build_responses_url(&self.base_origin, &preset_config.path);
        let model = preset_config
            .fixed_model
            .clone()
            .or_else(|| normalized_optional_text(model_override.map(str::to_string)))
            .unwrap_or_else(|| self.default_dynamic_model.clone());

        GrokRequestConfig {
            preset,
            endpoint,
            api_key: self.api_key.clone(),
            model,
        }
    }
}

async fn query_grok(
    config: &GrokRequestConfig,
    query: &str,
    platform: Option<&str>,
) -> Result<String, FunctionCallError> {
    let user_prompt = if let Some(platform) = platform {
        format!(
            "User request:\n{query}\n\nPrioritize this platform, source family, or ecosystem if relevant: {platform}"
        )
    } else {
        format!("User request:\n{query}")
    };
    complete_chat(config, GROK_SYSTEM_PROMPT, &user_prompt).await
}

async fn complete_chat(
    config: &GrokRequestConfig,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, FunctionCallError> {
    let client = build_reqwest_client();
    let response = client
        .post(&config.endpoint)
        .header(AUTHORIZATION, format!("Bearer {}", config.api_key))
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .header("originator", DEFAULT_GROK_APP_NAME)
        .json(&json!({
            "model": config.model,
            "instructions": system_prompt,
            "input": [
                {"role": "user", "content": user_prompt}
            ],
            "stream": false
        }))
        .send()
        .await
        .map_err(|err| FunctionCallError::RespondToModel(format!("Grok request failed: {err}")))?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<body unavailable>".to_string());
        return Err(FunctionCallError::RespondToModel(format!(
            "Grok request failed with status {status}: {body}"
        )));
    }

    let payload: Value = response.json().await.map_err(|err| {
        FunctionCallError::RespondToModel(format!("failed to parse Grok response: {err}"))
    })?;
    extract_output_text(&payload).ok_or_else(|| {
        FunctionCallError::RespondToModel(
            "Grok response did not include assistant content".to_string(),
        )
    })
}

fn render_output(
    session_id: String,
    request: &GrokRequestConfig,
    query: &str,
    answer: &str,
    notes: &[SourceNote],
) -> String {
    let mut lines = vec![
        format!("SESSION_ID={session_id}"),
        "MODE=grok".to_string(),
        format!("PRESET={}", preset_name(request.preset)),
        format!("MODEL={}", request.model),
        String::new(),
        "Query".to_string(),
        query.to_string(),
        String::new(),
        "Response".to_string(),
        answer.to_string(),
    ];

    if !notes.is_empty() {
        lines.push(String::new());
        lines.push("Source Notes".to_string());
        lines.extend(
            notes
                .iter()
                .enumerate()
                .map(|(index, note)| format!("{}. {}", index + 1, note.url)),
        );
    }

    lines.join("\n")
}

fn extract_unique_urls(text: &str) -> Vec<String> {
    static URL_RE: OnceLock<Regex> = OnceLock::new();
    let regex = URL_RE.get_or_init(|| {
        Regex::new(r#"https?://[^\s<>"'`，。、；：！？》）】)]+"#).expect("valid url regex")
    });

    let mut urls = Vec::new();
    for capture in regex.find_iter(text) {
        let url = capture
            .as_str()
            .trim_end_matches(['.', ',', ';', ':', '!', '?']);
        if !urls.iter().any(|seen| seen == url) {
            urls.push(url.to_string());
        }
    }
    urls
}

fn parse_preset_override(value: Option<&str>) -> Result<Option<GrokPresetId>, FunctionCallError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    normalize_preset_id(trimmed).map(Some).ok_or_else(|| {
        FunctionCallError::RespondToModel(format!(
            "unsupported Grok preset `{trimmed}`; use one of default, b42, expert41, thinking41 or a known alias"
        ))
    })
}

fn normalize_preset_id(value: &str) -> Option<GrokPresetId> {
    match value.trim() {
        "default" | "grokcodex" => Some(GrokPresetId::Default),
        "b42" | "grokcodexb42" | "grok-4.20-beta" => Some(GrokPresetId::B42),
        "expert41" | "grokcodex41expert" | "grok-4.1-expert" => Some(GrokPresetId::Expert41),
        "thinking41" | "grokcodex41thinking" | "grok-4.1-thinking" => {
            Some(GrokPresetId::Thinking41)
        }
        _ => None,
    }
}

fn build_responses_url(base_origin: &str, preset_path: &str) -> String {
    let origin = format!("{}/", base_origin.trim_end_matches('/'));
    let path = format!("{}/v1/responses", preset_path.trim_matches('/'));
    let url = reqwest::Url::parse(&origin)
        .expect("valid Grok base origin")
        .join(&path)
        .expect("valid Grok preset path");
    url.to_string()
}

fn extract_output_text(payload: &Value) -> Option<String> {
    if let Some(text) = payload.get("output_text").and_then(Value::as_str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let mut parts = Vec::new();
    for item in payload
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }

        for content in item
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if content.get("type").and_then(Value::as_str) == Some("output_text")
                && let Some(text) = content.get("text").and_then(Value::as_str)
            {
                parts.push(text);
            }
        }
    }

    let joined = parts.join("").trim().to_string();
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

fn preset_name(preset: GrokPresetId) -> &'static str {
    match preset {
        GrokPresetId::Default => "default",
        GrokPresetId::B42 => "b42",
        GrokPresetId::Expert41 => "expert41",
        GrokPresetId::Thinking41 => "thinking41",
    }
}

fn normalized_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

const fn default_source_note_limit() -> usize {
    DEFAULT_SOURCE_NOTE_LIMIT
}

fn clamp_source_note_limit(limit: usize) -> usize {
    if limit == 0 {
        DEFAULT_SOURCE_NOTE_LIMIT
    } else {
        limit.min(MAX_SOURCE_NOTE_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::make_session_and_context;
    use crate::config::test_config;
    use crate::turn_diff_tracker::TurnDiffTracker;
    use codex_config::types::GrokConfig;
    use codex_config::types::GrokPresetId;
    use serial_test::serial;
    use std::env;
    use std::ffi::OsStr;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    struct EnvVarGuard {
        key: &'static str,
        original: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &OsStr) -> Self {
            let original = env::var_os(key);
            unsafe {
                env::set_var(key, value);
            }
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => unsafe {
                    env::set_var(self.key, value);
                },
                None => unsafe {
                    env::remove_var(self.key);
                },
            }
        }
    }

    async fn invocation(
        tool_name: &str,
        arguments: serde_json::Value,
        config: Option<crate::config::Config>,
    ) -> ToolInvocation {
        let (session, mut turn) = make_session_and_context().await;
        if let Some(config) = config {
            turn.config = Arc::new(config);
        }
        ToolInvocation {
            session: Arc::new(session),
            turn: Arc::new(turn),
            tracker: Arc::new(Mutex::new(TurnDiffTracker::default())),
            call_id: "call-1".to_string(),
            tool_name: tool_name.to_string(),
            tool_namespace: None,
            payload: ToolPayload::Function {
                arguments: arguments.to_string(),
            },
        }
    }

    #[tokio::test]
    #[serial(grok_tool_env)]
    async fn grok_returns_answer_and_sources() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/grokcodexb42/v1/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "output_text": "Paris is the capital of France. Sources: https://example.com/france https://example.com/paris"
            })))
            .mount(&server)
            .await;

        let _api_key = EnvVarGuard::set("GROK_API_KEY", OsStr::new("test-key"));
        let mut config = test_config();
        config.grok.base_origin = server.uri();

        let output = GrokHandler
            .handle(
                invocation(
                    GROK_TOOL_NAME,
                    json!({"query": "What is the capital of France?"}),
                    Some(config),
                )
                .await,
            )
            .await
            .expect("grok tool should succeed");

        let text = output.into_text();
        assert!(text.contains("SESSION_ID="));
        assert!(text.contains("MODE=grok"));
        assert!(text.contains("PRESET=b42"));
        assert!(text.contains("Paris is the capital of France"));
        assert!(text.contains("https://example.com/france"));
        assert!(text.contains("https://example.com/paris"));
    }

    #[tokio::test]
    #[serial(grok_tool_env)]
    async fn grok_prefers_configured_preset_catalog_and_models() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/custom-thinking/v1/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "output_text": "Research plan with source https://example.com/plan"
            })))
            .mount(&server)
            .await;

        let _api_key = EnvVarGuard::set("MY_GROK_KEY", OsStr::new("test-key"));
        let mut config = test_config();
        config.grok = GrokConfig::default();
        config.grok.base_origin = server.uri();
        config.grok.api_key_env = "MY_GROK_KEY".to_string();
        config.grok.default_preset = GrokPresetId::Thinking41;
        config.grok.presets.thinking41.path = "/custom-thinking".to_string();
        config.grok.presets.thinking41.fixed_model = Some("custom-thinking-model".to_string());

        let output = GrokHandler
            .handle(
                invocation(
                    GROK_TOOL_NAME,
                    json!({"query": "Plan the research approach"}),
                    Some(config),
                )
                .await,
            )
            .await
            .expect("grok tool should use config defaults");

        let text = output.into_text();
        assert!(text.contains("PRESET=thinking41"));
        assert!(text.contains("MODEL=custom-thinking-model"));
        assert!(text.contains("https://example.com/plan"));
    }

    #[test]
    fn normalize_preset_id_accepts_canonical_ids_and_aliases() {
        assert_eq!(normalize_preset_id("b42"), Some(GrokPresetId::B42));
        assert_eq!(
            normalize_preset_id("grokcodex41thinking"),
            Some(GrokPresetId::Thinking41)
        );
        assert_eq!(
            normalize_preset_id("grok-4.1-expert"),
            Some(GrokPresetId::Expert41)
        );
        assert_eq!(normalize_preset_id("grok-4.1-fast"), None);
    }
}
