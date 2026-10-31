//! Integration tests that need a real `zsh`. They skip if it is not on PATH.

use be_shell::{Session, ShellState, zsh};
use std::{
  fs,
  path::PathBuf,
  sync::atomic::{AtomicU32, Ordering},
};

fn tmpdir(name: &str) -> PathBuf {
  static N: AtomicU32 = AtomicU32::new(0);
  let d = std::env::temp_dir().join(format!(
    "be-shell-it-{}-{}-{name}",
    std::process::id(),
    N.fetch_add(1, Ordering::Relaxed)
  ));
  let _ = fs::remove_dir_all(&d);
  fs::create_dir_all(&d).unwrap();
  d.canonicalize().unwrap()
}

macro_rules! need_zsh {
  () => {
    if !zsh::zsh_available() {
      eprintln!("zsh not found, skipping");
      return;
    }
  };
}

/// A session whose snapshot came from a throwaway ZDOTDIR rc file.
fn session(name: &str, rc: &str) -> (Session, PathBuf) {
  let dir = tmpdir(name);
  let zdot = dir.join("zdot");
  fs::create_dir_all(&zdot).unwrap();
  fs::write(zdot.join(".zshrc"), rc).unwrap();
  let work = dir.join("work");
  fs::create_dir_all(&work).unwrap();
  let mut state = ShellState::from_process();
  state.cwd = work.clone();
  state.env.retain(|(k, _)| k != "ZDOTDIR");
  state.env.push(("ZDOTDIR".into(), zdot.to_string_lossy().into_owned()));
  let mut s = Session::new(dir.join("session"), state).unwrap();
  let r = s.snapshot_blocking().unwrap();
  assert!(r.state_updated, "snapshot failed: {}", String::from_utf8_lossy(&r.stderr));
  (s, work)
}

fn run(s: &mut Session, line: &str) -> (Option<i32>, String) {
  let r = s.run_blocking(line).unwrap();
  (r.status, String::from_utf8_lossy(&r.stdout).trim_end().to_string())
}

#[test]
fn rc_file_state_is_snapshotted() {
  need_zsh!();
  let (mut s, _) = session(
    "rc",
    "alias rcal='echo from-rc-alias'\nrcfn() { echo rc-fn $1 }\nexport RC_EXPORT=exp\nRC_PLAIN=plain\nsetopt nullglob\nRC_ARR=(a b)\n",
  );
  assert_eq!(run(&mut s, "rcal"), (Some(0), "from-rc-alias".into()));
  assert_eq!(run(&mut s, "rcfn 1"), (Some(0), "rc-fn 1".into()));
  assert_eq!(
    run(&mut s, "echo $RC_EXPORT $RC_PLAIN ${RC_ARR[2]}"),
    (Some(0), "exp plain b".into())
  );
  assert_eq!(run(&mut s, "[[ -o nullglob ]] && echo yes"), (Some(0), "yes".into()));
}

#[test]
fn roundtrip() {
  need_zsh!();
  let (mut s, work) = session("rt", "");
  fs::create_dir(work.join("sub")).unwrap();
  fs::write(work.join("sub/env.sh"), "export FROM_SRC=1\nsrcvar=2\n").unwrap();

  run(&mut s, "cd sub");
  assert_eq!(s.state.cwd, work.join("sub"));
  assert_eq!(run(&mut s, "pwd").1, work.join("sub").to_string_lossy());

  run(&mut s, "export EXP=hello world");
  assert_eq!(s.state.env_get("EXP"), Some("hello"));
  run(&mut s, "export EXP2='two words'");
  assert_eq!(s.state.env_get("EXP2"), Some("two words"));
  assert_eq!(run(&mut s, "echo $EXP2").1, "two words");

  run(&mut s, "alias hi='echo hi-alias'");
  assert_eq!(run(&mut s, "hi there").1, "hi-alias there");

  run(&mut s, "greet() { echo \"hello $1\" }");
  assert_eq!(run(&mut s, "greet bob").1, "hello bob");

  run(&mut s, "setopt nullglob");
  assert_eq!(run(&mut s, "echo nomatch*").1, "");
  run(&mut s, "unsetopt nullglob");
  assert_eq!(run(&mut s, "[[ -o nullglob ]] || echo off").1, "off");

  run(&mut s, "plain='a  b'; arr=(x 'y z'); typeset -A as; as=(k 'v v')");
  assert_eq!(run(&mut s, "echo \"$plain|${arr[2]}|${as[k]}|${#arr}\"").1, "a  b|y z|v v|2");

  run(&mut s, "source env.sh");
  assert_eq!(run(&mut s, "echo $FROM_SRC $srcvar").1, "1 2");

  run(&mut s, "unset EXP2");
  assert_eq!(s.state.env_get("EXP2"), None);

  run(&mut s, "umask 027");
  assert_eq!(run(&mut s, "umask").1, "027");

  run(&mut s, "pushd /tmp > /dev/null");
  run(&mut s, "cd -");
  assert_eq!(s.state.cwd, work.join("sub"));

  // The internal variables are never part of the state.
  for (k, _) in &s.state.env {
    assert!(!matches!(k.as_str(), "BE_LINE" | "BE_OUT" | "BE_SCRIPT" | "BE_BASELINE" | "_"), "{k}");
  }
}

#[test]
fn exit_status_preserved() {
  need_zsh!();
  let (mut s, _) = session("status", "");
  assert_eq!(run(&mut s, "(exit 7)").0, Some(7));
  assert_eq!(run(&mut s, "false").0, Some(1));
  assert_eq!(run(&mut s, "true").0, Some(0));
  let r = s.run_blocking("cd /; exit 3").unwrap();
  assert_eq!(r.status, Some(3));
  assert!(r.state_updated);
  assert_eq!(s.state.cwd, PathBuf::from("/"));
}

#[test]
fn state_unchanged_when_hook_never_ran() {
  need_zsh!();
  let (mut s, work) = session("kill", "");
  run(&mut s, "export KEEP=1");
  let before = s.state.clone();
  let r = s.run_blocking("cd /; export LOST=1; kill -9 $$").unwrap();
  assert_eq!(r.status, None);
  assert!(!r.state_updated);
  assert_eq!(s.state, before);
  assert_eq!(s.state.cwd, work);

  let r = s.run_blocking("cd /; exec true").unwrap();
  assert!(!r.state_updated);
  assert_eq!(s.state, before);
}

#[test]
fn line_is_not_quoted_or_parsed() {
  need_zsh!();
  let (mut s, _) = session("quote", "");
  assert_eq!(run(&mut s, "echo \"it's\" 'a\"b' $'tab\\there'").1, "it's a\"b tab\there");
  assert_eq!(run(&mut s, "echo one\necho two").1, "one\ntwo");
  assert_eq!(run(&mut s, "echo /etc/hostn* | tr a-z A-Z").1, "/ETC/HOSTNAME");
  // Internal variables are not exported to the user's commands.
  assert_eq!(run(&mut s, "env | grep -c '^BE_'").1, "0");
}

#[test]
fn user_exit_trap_does_not_break_dump() {
  need_zsh!();
  let (mut s, _) = session("trap", "");
  assert_eq!(run(&mut s, "trap 'echo bye' EXIT; export T=1"), (Some(0), "bye".into()));
  assert_eq!(s.state.env_get("T"), Some("1"));
}

#[test]
fn state_does_not_grow() {
  need_zsh!();
  let (mut s, _) = session("grow", "alias a=b\nf() { :; }\n");
  run(&mut s, "true");
  let len = s.state.script.len();
  let env = s.state.env.len();
  for _ in 0..3 {
    run(&mut s, "true");
  }
  assert_eq!(s.state.script.len(), len);
  assert_eq!(s.state.env.len(), env);
}

#[test]
fn spec_is_plain_data() {
  let state = ShellState {
    cwd:    "/tmp".into(),
    env:    vec![("A".into(), "1".into())],
    script: String::new(),
  };
  let spec = zsh::command_spec(
    &state,
    "echo 'hi'",
    "/s/state.zsh".as_ref(),
    "/s/run-1".as_ref(),
    "/s/baseline.txt".as_ref(),
  );
  assert_eq!(spec.program, "zsh");
  assert_eq!(&spec.args[..2], ["-f", "-c"]);
  assert_eq!(spec.cwd, PathBuf::from("/tmp"));
  let get = |k: &str| spec.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
  assert_eq!(get("BE_LINE"), Some("echo 'hi'"));
  assert_eq!(get("BE_OUT"), Some("/s/run-1"));
  assert_eq!(get("BE_SCRIPT"), Some("/s/state.zsh"));
  assert_eq!(get("PWD"), Some("/tmp"));
  assert_eq!(get("A"), Some("1"));
  // The line is never interpolated into the script.
  assert!(!spec.args.iter().any(|a| a.contains("echo 'hi'")));
}
