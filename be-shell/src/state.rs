//! Shell state that persists between commands.
//!
//! Only `cwd` and `env` are understood by Rust. `script` is an opaque zsh
//! script (aliases, functions, options, non-exported variables, ...) that is
//! sourced before every command and is never parsed.

use std::{
  fs, io,
  path::{Path, PathBuf},
};

/// Environment variables used to talk to the zsh helper scripts. They are never
/// stored in the state.
pub const INTERNAL_VARS: &[&str] = &["BE_LINE", "BE_OUT", "BE_SCRIPT", "BE_BASELINE"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellState {
  pub cwd:    PathBuf,
  pub env:    Vec<(String, String)>,
  /// Opaque zsh script, sourced before every command.
  pub script: String,
}

impl ShellState {
  pub fn new(cwd: impl Into<PathBuf>) -> Self {
    ShellState { cwd: cwd.into(), env: vec![], script: String::new() }
  }

  /// Builds a state from the current process environment and directory, with
  /// an empty script.
  pub fn from_process() -> Self {
    let mut state = ShellState::new(std::env::current_dir().unwrap_or_else(|_| "/".into()));
    state.env = std::env::vars().collect();
    state.env.retain(|(k, _)| !INTERNAL_VARS.contains(&k.as_str()));
    state
  }

  pub fn env_get(&self, key: &str) -> Option<&str> {
    self.env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
  }

  /// Parses the three files produced by the dump script.
  ///
  /// `env` is `NAME=value` entries separated by NUL. Non-UTF-8 bytes are
  /// replaced. Entries that should not be restored (`_`, internal variables)
  /// are dropped, and `SHLVL` is decremented, because every `zsh` we spawn
  /// increments it again.
  pub fn from_dump(cwd: &[u8], env: &[u8], script: &[u8]) -> ShellState {
    let cwd = PathBuf::from(String::from_utf8_lossy(cwd).into_owned());
    ShellState { cwd, env: parse_env(env), script: String::from_utf8_lossy(script).into_owned() }
  }
}

/// Parses `env -0` output.
pub fn parse_env(env: &[u8]) -> Vec<(String, String)> {
  let mut out = vec![];
  for entry in env.split(|b| *b == 0) {
    if entry.is_empty() {
      continue;
    }
    let entry = String::from_utf8_lossy(entry);
    let Some((k, v)) = entry.split_once('=') else { continue };
    if k.is_empty() || k == "_" || INTERNAL_VARS.contains(&k) {
      continue;
    }
    let v = if k == "SHLVL" {
      match v.parse::<i64>() {
        Ok(n) => (n - 1).max(0).to_string(),
        Err(_) => v.to_string(),
      }
    } else {
      v.to_string()
    };
    out.push((k.to_string(), v));
  }
  out
}

/// A directory the dump script writes into (one per command run).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DumpDir {
  path: PathBuf,
}

impl DumpDir {
  /// Creates (or empties) the directory.
  pub fn create(path: impl Into<PathBuf>) -> io::Result<DumpDir> {
    let path = path.into();
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path)?;
    Ok(DumpDir { path })
  }

  pub fn path(&self) -> &Path { &self.path }

  /// Reads the dump. Returns `None` if the `done` marker is missing, which
  /// means the EXIT hook never finished (SIGKILL, `exec`, ...). The caller
  /// should keep its previous state in that case.
  pub fn read(&self) -> io::Result<Option<ShellState>> {
    if !self.path.join("done").exists() {
      return Ok(None);
    }
    let cwd = fs::read(self.path.join("cwd"))?;
    let env = fs::read(self.path.join("env"))?;
    let script = fs::read(self.path.join("script"))?;
    if cwd.is_empty() {
      return Ok(None);
    }
    Ok(Some(ShellState::from_dump(&cwd, &env, &script)))
  }

  pub fn remove(&self) { let _ = fs::remove_dir_all(&self.path); }
}

#[cfg(test)]
mod tests {
  use super::*;
  use expect_test::expect;

  #[test]
  fn parse_env_basic() {
    let env = parse_env(b"A=1\0B=two=2\0_=/usr/bin/env\0BE_LINE=x\0SHLVL=3\0multi=a\nb\0bad\0");
    expect![@r#"
        [
            (
                "A",
                "1",
            ),
            (
                "B",
                "two=2",
            ),
            (
                "SHLVL",
                "2",
            ),
            (
                "multi",
                "a\nb",
            ),
        ]
    "#]
    .assert_debug_eq(&env);
  }

  #[test]
  fn shlvl_never_negative() {
    assert_eq!(parse_env(b"SHLVL=0\0"), vec![("SHLVL".to_string(), "0".to_string())]);
  }

  #[test]
  fn dump_dir_without_marker_is_none() {
    let dir =
      DumpDir::create(std::env::temp_dir().join(format!("be-shell-t1-{}", std::process::id())))
        .unwrap();
    fs::write(dir.path().join("cwd"), "/tmp").unwrap();
    fs::write(dir.path().join("env"), "A=1\0").unwrap();
    fs::write(dir.path().join("script"), "").unwrap();
    assert_eq!(dir.read().unwrap(), None);
    fs::write(dir.path().join("done"), "ok\n").unwrap();
    let s = dir.read().unwrap().unwrap();
    assert_eq!(s.cwd, PathBuf::from("/tmp"));
    assert_eq!(s.env_get("A"), Some("1"));
    dir.remove();
  }
}
