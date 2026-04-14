#![cfg(not(debug_assertions))]

use chrono::DateTime;
use chrono::Duration;
use chrono::Utc;
use codex_login::default_client::create_client;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use crate::legacy_core::branding;
use crate::legacy_core::config::Config;
use crate::version::CODEX_CLI_VERSION;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodexUpdateNotice {
    pub current_version: String,
    pub latest_version: String,
    pub release_notes_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamReleaseGapNotice {
    pub current_version: String,
    pub latest_version: String,
    pub releases_ahead: usize,
    pub release_notes_url: String,
}

pub fn godex_current_version() -> String {
    branding::APP_VERSION.to_string()
}

pub fn get_godex_update_notice(config: &Config) -> Option<GodexUpdateNotice> {
    if !config.check_for_update_on_startup || !config.godex_updates.enabled {
        return None;
    }
    let release_repo = effective_godex_release_repo(config)?;
    let updates_file = updates_filepath(config);
    let state = read_updates_state(&updates_file).ok();
    refresh_updates_if_needed(config, &updates_file, state.as_ref());

    let info = state?.godex?;
    if is_newer(&info.latest_version, branding::APP_VERSION).unwrap_or(false) {
        Some(GodexUpdateNotice {
            current_version: godex_current_version(),
            latest_version: info.latest_version,
            release_notes_url: format!("https://github.com/{}/releases/latest", release_repo),
        })
    } else {
        None
    }
}

pub fn get_godex_update_notice_for_popup(config: &Config) -> Option<GodexUpdateNotice> {
    let notice = get_godex_update_notice(config)?;
    let updates_file = updates_filepath(config);
    if let Ok(state) = read_updates_state(&updates_file)
        && state
            .godex
            .and_then(|info| info.dismissed_version)
            .as_deref()
            == Some(notice.latest_version.as_str())
    {
        return None;
    }
    Some(notice)
}

pub fn get_upstream_release_gap_notice(config: &Config) -> Option<UpstreamReleaseGapNotice> {
    if !config.check_for_update_on_startup
        || !config.upstream_updates.enabled
        || config.upstream_updates.repo_root.is_none()
    {
        return None;
    }

    let updates_file = updates_filepath(config);
    let state = read_updates_state(&updates_file).ok();
    refresh_updates_if_needed(config, &updates_file, state.as_ref());

    let info = state?.upstream?;
    if info.releases_ahead == 0 {
        return None;
    }

    Some(UpstreamReleaseGapNotice {
        current_version: info.current_version,
        latest_version: info.latest_version,
        releases_ahead: info.releases_ahead,
        release_notes_url: upstream_release_notes_url(config),
    })
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct UpdatesState {
    #[serde(default)]
    godex: Option<GodexVersionInfo>,
    #[serde(default)]
    upstream: Option<UpstreamReleaseGapInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct GodexVersionInfo {
    latest_version: String,
    last_checked_at: DateTime<Utc>,
    #[serde(default)]
    dismissed_version: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct UpstreamReleaseGapInfo {
    current_version: String,
    latest_version: String,
    releases_ahead: usize,
    last_checked_at: DateTime<Utc>,
}

const UPDATES_FILENAME: &str = "updates.json";
const UPDATE_CHECK_INTERVAL_HOURS: i64 = 20;

#[derive(Deserialize, Debug, Clone)]
struct ReleaseInfo {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

fn updates_filepath(config: &Config) -> PathBuf {
    config.codex_home.join(UPDATES_FILENAME).to_path_buf()
}

fn read_updates_state(updates_file: &Path) -> anyhow::Result<UpdatesState> {
    let contents = std::fs::read_to_string(updates_file)?;
    Ok(serde_json::from_str(&contents)?)
}

fn refresh_updates_if_needed(config: &Config, updates_file: &Path, state: Option<&UpdatesState>) {
    if !needs_refresh(config, state) {
        return;
    }

    let config = config.clone();
    let updates_file = updates_file.to_path_buf();
    tokio::spawn(async move {
        refresh_updates(&config, &updates_file)
            .await
            .inspect_err(|e| tracing::error!("Failed to refresh updates: {e}"))
    });
}

fn needs_refresh(config: &Config, state: Option<&UpdatesState>) -> bool {
    let now = Utc::now();
    let has_godex_release_repo = effective_godex_release_repo(config).is_some();

    let godex_stale = config.godex_updates.enabled
        && has_godex_release_repo
        && state
            .and_then(|state| state.godex.as_ref())
            .is_none_or(|info| {
                info.last_checked_at < now - Duration::hours(UPDATE_CHECK_INTERVAL_HOURS)
            });

    let upstream_stale = config.upstream_updates.enabled
        && config.upstream_updates.repo_root.is_some()
        && state
            .and_then(|state| state.upstream.as_ref())
            .is_none_or(|info| {
                info.last_checked_at < now - Duration::hours(UPDATE_CHECK_INTERVAL_HOURS)
            });

    godex_stale || upstream_stale
}

async fn refresh_updates(config: &Config, updates_file: &Path) -> anyhow::Result<()> {
    let prev_state = read_updates_state(updates_file).unwrap_or_default();
    let godex = refresh_godex_update_info(config, prev_state.godex.as_ref()).await?;
    let upstream = refresh_upstream_release_gap(config).await?;
    let state = UpdatesState { godex, upstream };

    let json_line = format!("{}\n", serde_json::to_string(&state)?);
    if let Some(parent) = updates_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(updates_file, json_line).await?;
    Ok(())
}

async fn refresh_godex_update_info(
    config: &Config,
    previous: Option<&GodexVersionInfo>,
) -> anyhow::Result<Option<GodexVersionInfo>> {
    if !config.godex_updates.enabled {
        return Ok(None);
    }

    let Some(release_repo) = effective_godex_release_repo(config) else {
        return Ok(None);
    };

    let latest_version = fetch_latest_release_version(&release_repo).await?;
    Ok(Some(GodexVersionInfo {
        latest_version,
        last_checked_at: Utc::now(),
        dismissed_version: previous.and_then(|info| info.dismissed_version.clone()),
    }))
}

async fn refresh_upstream_release_gap(
    config: &Config,
) -> anyhow::Result<Option<UpstreamReleaseGapInfo>> {
    if !config.upstream_updates.enabled {
        return Ok(None);
    }

    let Some(current_version) = source_repo_base_version(config) else {
        return Ok(None);
    };

    let releases = fetch_release_versions(&config.upstream_updates.release_repo).await?;
    let releases_ahead = count_releases_ahead(&releases, &current_version).unwrap_or_default();
    let latest_version = releases
        .iter()
        .next_back()
        .map(|version| format_version(*version))
        .unwrap_or_else(|| current_version.clone());

    Ok(Some(UpstreamReleaseGapInfo {
        current_version,
        latest_version,
        releases_ahead,
        last_checked_at: Utc::now(),
    }))
}

async fn fetch_latest_release_version(release_repo: &str) -> anyhow::Result<String> {
    let ReleaseInfo { tag_name, .. } = create_client()
        .get(format!(
            "https://api.github.com/repos/{release_repo}/releases/latest"
        ))
        .send()
        .await?
        .error_for_status()?
        .json::<ReleaseInfo>()
        .await?;
    extract_version_from_release_tag(&tag_name)
}

async fn fetch_release_versions(release_repo: &str) -> anyhow::Result<BTreeSet<(u64, u64, u64)>> {
    let releases = create_client()
        .get(format!(
            "https://api.github.com/repos/{release_repo}/releases?per_page=100"
        ))
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<ReleaseInfo>>()
        .await?;

    Ok(releases
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| extract_version_from_release_tag(&release.tag_name).ok())
        .filter_map(|version| parse_version(&version))
        .collect())
}

fn upstream_release_notes_url(config: &Config) -> String {
    format!(
        "https://github.com/{}/releases/latest",
        config.upstream_updates.release_repo
    )
}

fn effective_godex_release_repo(config: &Config) -> Option<String> {
    config
        .godex_updates
        .release_repo
        .as_deref()
        .map(str::trim)
        .filter(|repo| !repo.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            config
                .upstream_updates
                .repo_root
                .as_deref()
                .and_then(detect_origin_github_repo)
        })
        .or_else(|| Some(branding::APP_GITHUB_REPO.to_string()))
}

fn detect_origin_github_repo(repo_root: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let remote = String::from_utf8(output.stdout).ok()?;
    parse_github_repo(remote.trim())
}

fn parse_github_repo(remote: &str) -> Option<String> {
    let remote = remote.trim();
    let normalized = remote
        .strip_prefix("git@github.com:")
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))
        .or_else(|| remote.strip_prefix("https://github.com/"))
        .or_else(|| remote.strip_prefix("http://github.com/"))?;
    let normalized = normalized.trim_end_matches(".git").trim_matches('/');
    let mut parts = normalized.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

fn source_repo_base_version(config: &Config) -> Option<String> {
    let repo_root = config.upstream_updates.repo_root.as_ref()?;
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args([
            "describe",
            "--tags",
            "--match",
            "rust-v*",
            "--abbrev=0",
            "HEAD",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let tag = String::from_utf8(output.stdout).ok()?;
    extract_version_from_release_tag(tag.trim()).ok()
}

pub async fn dismiss_godex_version(config: &Config, version: &str) -> anyhow::Result<()> {
    let updates_file = updates_filepath(config);
    let mut state = match read_updates_state(&updates_file) {
        Ok(state) => state,
        Err(_) => return Ok(()),
    };
    let Some(godex) = state.godex.as_mut() else {
        return Ok(());
    };
    godex.dismissed_version = Some(version.to_string());
    let json_line = format!("{}\n", serde_json::to_string(&state)?);
    if let Some(parent) = updates_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(updates_file, json_line).await?;
    Ok(())
}

fn extract_version_from_release_tag(tag_name: &str) -> anyhow::Result<String> {
    for candidate in [
        tag_name,
        tag_name.strip_prefix("rust-v").unwrap_or(tag_name),
        tag_name.strip_prefix("godex-v").unwrap_or(tag_name),
        tag_name.strip_prefix('v').unwrap_or(tag_name),
    ] {
        if parse_version(candidate).is_some() {
            return Ok(candidate.to_string());
        }
    }

    if let Some(candidate) = tag_name
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .find(|candidate| parse_version(candidate).is_some())
    {
        return Ok(candidate.to_string());
    }

    anyhow::bail!("Failed to parse release tag '{tag_name}'");
}

fn count_releases_ahead(
    releases: &BTreeSet<(u64, u64, u64)>,
    current_version: &str,
) -> Option<usize> {
    let current = parse_version(current_version)?;
    Some(
        releases
            .iter()
            .filter(|version| **version > current)
            .count(),
    )
}

fn format_version(version: (u64, u64, u64)) -> String {
    format!("{}.{}.{}", version.0, version.1, version.2)
}

fn is_newer(latest: &str, current: &str) -> Option<bool> {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => Some(l > c),
        _ => None,
    }
}

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut iter = v.trim().split('.');
    let maj = iter.next()?.parse::<u64>().ok()?;
    let min = iter.next()?.parse::<u64>().ok()?;
    let pat = iter.next()?.parse::<u64>().ok()?;
    Some((maj, min, pat))
}

pub fn get_upgrade_version(config: &Config) -> Option<String> {
    if is_source_build_version(CODEX_CLI_VERSION) {
        return None;
    }

    get_godex_update_notice(config).map(|notice| notice.latest_version)
}

fn extract_version_from_latest_tag(latest_tag_name: &str) -> anyhow::Result<String> {
    extract_version_from_release_tag(latest_tag_name)
}

/// Returns the latest version to show in a popup, if it should be shown.
/// This respects the user's dismissal choice for the current latest version.
pub fn get_upgrade_version_for_popup(config: &Config) -> Option<String> {
    if is_source_build_version(CODEX_CLI_VERSION) {
        return None;
    }

    get_godex_update_notice_for_popup(config).map(|notice| notice.latest_version)
}

/// Persist a dismissal for the current latest version so we don't show
/// the update popup again for this version.
pub async fn dismiss_version(config: &Config, version: &str) -> anyhow::Result<()> {
    dismiss_godex_version(config, version).await
}

fn is_source_build_version(version: &str) -> bool {
    parse_version(version) == Some((0, 0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_version_from_release_tags() {
        assert_eq!(
            extract_version_from_release_tag("rust-v1.5.0").expect("failed to parse version"),
            "1.5.0"
        );
        assert_eq!(
            extract_version_from_release_tag("godex-v0.1.0").expect("failed to parse version"),
            "0.1.0"
        );
        assert_eq!(
            extract_version_from_release_tag("v2.3.4").expect("failed to parse version"),
            "2.3.4"
        );
    }

    #[test]
    fn latest_tag_without_semver_is_invalid() {
        assert!(extract_version_from_release_tag("release-candidate").is_err());
    }

    #[test]
    fn counts_upstream_releases_ahead() {
        let releases = BTreeSet::from([(0, 99, 0), (1, 0, 0), (1, 1, 0)]);
        assert_eq!(count_releases_ahead(&releases, "0.99.0"), Some(2));
        assert_eq!(count_releases_ahead(&releases, "1.1.0"), Some(0));
    }

    #[test]
    fn prerelease_version_is_not_considered_newer() {
        assert_eq!(is_newer("0.11.0-beta.1", "0.11.0"), None);
        assert_eq!(is_newer("1.0.0-rc.1", "1.0.0"), None);
    }

    #[test]
    fn plain_semver_comparisons_work() {
        assert_eq!(is_newer("0.11.1", "0.11.0"), Some(true));
        assert_eq!(is_newer("0.11.0", "0.11.1"), Some(false));
        assert_eq!(is_newer("1.0.0", "0.9.9"), Some(true));
        assert_eq!(is_newer("0.9.9", "1.0.0"), Some(false));
    }

    #[test]
    fn parses_github_remote_urls() {
        assert_eq!(
            parse_github_repo("git@github.com:LeonSGP43/godex.git"),
            Some("LeonSGP43/godex".to_string())
        );
        assert_eq!(
            parse_github_repo("https://github.com/LeonSGP43/godex"),
            Some("LeonSGP43/godex".to_string())
        );
        assert_eq!(
            parse_github_repo("ssh://git@github.com/LeonSGP43/godex.git"),
            Some("LeonSGP43/godex".to_string())
        );
        assert_eq!(
            parse_github_repo("https://gitlab.com/LeonSGP43/godex"),
            None
        );
    }
}
