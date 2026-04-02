use std::path::Path;

use anyhow::Result;
use predicates::str::contains;
use tempfile::TempDir;

fn godex_command(home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("godex")?);
    cmd.env("HOME", home)
        .env_remove("CODEX_HOME")
        .env_remove("GODEX_HOME");
    Ok(cmd)
}

#[test]
fn godex_g_creates_isolated_home_on_first_run() -> Result<()> {
    let home = TempDir::new()?;
    let expected_godex_home = home.path().join(".godex");

    let mut cmd = godex_command(home.path())?;
    cmd.args(["-g", "--version"]).assert().success();

    assert!(expected_godex_home.is_dir());
    assert!(!home.path().join(".codex").exists());

    Ok(())
}

#[test]
fn godex_still_rejects_missing_explicit_codex_home_for_config_commands() -> Result<()> {
    let home = TempDir::new()?;
    let missing_codex_home = home.path().join("missing-codex-home");

    let mut cmd = godex_command(home.path())?;
    cmd.env("CODEX_HOME", &missing_codex_home)
        .args(["features", "enable", "unified_exec"])
        .assert()
        .failure()
        .stderr(contains("CODEX_HOME points to"));

    Ok(())
}
