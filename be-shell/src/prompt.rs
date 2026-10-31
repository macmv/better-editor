//! What a prompt shows, captured when a line is submitted.

use std::{
  fs,
  path::{Path, PathBuf},
  sync::{Arc, OnceLock},
  time::{Duration, SystemTime},
};

use git2::{Repository, RepositoryState, Status, StatusOptions};

/// Called from a background thread when something a prompt shows has been
/// computed, so that the UI can redraw.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// What `HEAD` points at. Read straight from `.git/HEAD`, so it is cheap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHead {
  Branch(String),
  /// A short commit id.
  Detached(String),
}

/// The parts of the git state that take a while to compute.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitStatus {
  /// A merge, rebase, etc. in progress, as zim's git-info names it (in
  /// upper case): `MERGE`, `REBASE-I`, `CHERRY-SEQ`, ...
  pub action:    Option<String>,
  pub stashed:   bool,
  /// Changes in the working tree to tracked files. Untracked files are not
  /// counted.
  pub unindexed: bool,
  /// Changes in the index.
  pub indexed:   bool,
  /// Commits not on the upstream branch.
  pub ahead:     bool,
  /// Commits on the upstream branch that we don't have.
  pub behind:    bool,
}

#[derive(Debug, Clone)]
pub struct PromptContext {
  pub cwd:       PathBuf,
  pub home:      Option<PathBuf>,
  pub git:       Option<GitHead>,
  /// Computed on a background thread (it can be slow in a big repo); see
  /// [`PromptContext::git_status`].
  git_status:    Arc<OnceLock<GitStatus>>,
  /// `$VIRTUAL_ENV`'s directory name.
  pub venv:      Option<String>,
  pub user:      Option<String>,
  pub host:      Option<String>,
  /// `$SSH_TTY` is set.
  pub ssh:       bool,
  /// Running as root.
  pub root:      bool,
  /// Exit status of the previous command.
  pub last_exit: Option<i32>,
  /// How long the previous command took.
  pub duration:  Option<Duration>,
  pub time:      SystemTime,
}

impl PromptContext {
  /// A context with nothing but a working directory.
  pub fn bare(cwd: &Path) -> PromptContext {
    PromptContext {
      cwd:        cwd.to_path_buf(),
      home:       None,
      git:        None,
      git_status: Arc::new(OnceLock::new()),
      venv:       None,
      user:       None,
      host:       None,
      ssh:        false,
      root:       false,
      last_exit:  None,
      duration:   None,
      time:       SystemTime::now(),
    }
  }

  /// Gathers the context. Never blocks on git: the status is computed on a
  /// thread and shows up later in [`PromptContext::git_status`], after which
  /// `wake` is called.
  pub fn capture(
    cwd: &Path,
    env: &[(String, String)],
    last_exit: Option<i32>,
    duration: Option<Duration>,
    wake: Option<Wake>,
  ) -> PromptContext {
    let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let mut ctx = PromptContext::bare(cwd);
    ctx.git = git_head(cwd);
    if ctx.git.is_some() {
      let slot = ctx.git_status.clone();
      let cwd = cwd.to_path_buf();
      let spawned = std::thread::Builder::new().name("be-shell-git".into()).spawn(move || {
        if let Some(status) = compute_status(&cwd) {
          let _ = slot.set(status);
          if let Some(wake) = wake {
            wake();
          }
        }
      });
      if let Err(e) = spawned {
        log::warn!("failed to spawn git status thread: {e}");
      }
    }

    ctx.home = get("HOME").map(PathBuf::from);
    ctx.venv = get("VIRTUAL_ENV")
      .and_then(|v| Path::new(&v).file_name().map(|n| n.to_string_lossy().into_owned()));
    ctx.user = get("USER").or_else(|| get("LOGNAME"));
    ctx.host = get("HOSTNAME").or_else(|| {
      fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
    });
    ctx.ssh = get("SSH_TTY").is_some_and(|v| !v.is_empty());
    // SAFETY: geteuid has no preconditions.
    ctx.root = unsafe { libc::geteuid() } == 0;
    ctx.last_exit = last_exit;
    ctx.duration = duration;
    ctx
  }

  /// The slow part of the git state, once it has been computed.
  pub fn git_status(&self) -> Option<&GitStatus> { self.git_status.get() }

  /// Fills in the git status, as the background thread would. Does nothing if
  /// it is already set.
  pub fn set_git_status(&self, status: GitStatus) { let _ = self.git_status.set(status); }
}

/// Finds what `HEAD` points at, without opening the repo.
pub fn git_head(cwd: &Path) -> Option<GitHead> {
  for dir in cwd.ancestors() {
    let dot_git = dir.join(".git");
    let git_dir = if dot_git.is_dir() {
      dot_git
    } else if dot_git.is_file() {
      // A worktree or submodule: `gitdir: <path>`.
      let text = fs::read_to_string(&dot_git).ok()?;
      let path = text.trim().strip_prefix("gitdir:")?.trim();
      let path = Path::new(path);
      if path.is_absolute() { path.to_path_buf() } else { dir.join(path) }
    } else {
      continue;
    };
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    return Some(match head.strip_prefix("ref: ") {
      Some(r) => GitHead::Branch(r.strip_prefix("refs/heads/").unwrap_or(r).to_string()),
      None => GitHead::Detached(head.chars().take(7).collect()),
    });
  }
  None
}

/// The status as zim's git-info computes it.
pub fn compute_status(cwd: &Path) -> Option<GitStatus> {
  let repo = Repository::discover(cwd).ok()?;
  let mut status = GitStatus {
    action: action_name(repo.state()).map(str::to_string),
    stashed: repo.find_reference("refs/stash").is_ok(),
    ..GitStatus::default()
  };

  let mut opts = StatusOptions::new();
  opts.include_untracked(false).include_ignored(false).exclude_submodules(true);
  if let Ok(statuses) = repo.statuses(Some(&mut opts)) {
    let index = Status::INDEX_NEW
      | Status::INDEX_MODIFIED
      | Status::INDEX_DELETED
      | Status::INDEX_RENAMED
      | Status::INDEX_TYPECHANGE;
    let worktree = Status::WT_MODIFIED
      | Status::WT_DELETED
      | Status::WT_TYPECHANGE
      | Status::WT_RENAMED
      | Status::CONFLICTED;
    for entry in statuses.iter() {
      status.indexed |= entry.status().intersects(index);
      status.unindexed |= entry.status().intersects(worktree);
    }
  }

  if let Ok(head) = repo.head()
    && head.is_branch()
    && let Some(local) = head.target()
    && let Ok(upstream) = git2::Branch::wrap(head).upstream()
    && let Some(remote) = upstream.get().target()
    && let Ok((ahead, behind)) = repo.graph_ahead_behind(local, remote)
  {
    status.ahead = ahead > 0;
    status.behind = behind > 0;
  }
  Some(status)
}

fn action_name(state: RepositoryState) -> Option<&'static str> {
  Some(match state {
    RepositoryState::Clean => return None,
    RepositoryState::Merge => "MERGE",
    RepositoryState::Revert => "REVERT",
    RepositoryState::RevertSequence => "REVERT-SEQ",
    RepositoryState::CherryPick => "CHERRY",
    RepositoryState::CherryPickSequence => "CHERRY-SEQ",
    RepositoryState::Bisect => "BISECT",
    RepositoryState::Rebase => "REBASE",
    RepositoryState::RebaseInteractive => "REBASE-I",
    RepositoryState::RebaseMerge => "REBASE-M",
    RepositoryState::ApplyMailbox => "AM",
    RepositoryState::ApplyMailboxOrRebase => "AM/REBASE",
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("be-shell-prompt-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
  }

  #[test]
  fn head_from_file() {
    let d = temp("head");
    fs::create_dir_all(d.join(".git")).unwrap();
    fs::create_dir_all(d.join("a/b")).unwrap();
    fs::write(d.join(".git/HEAD"), "ref: refs/heads/feat/x\n").unwrap();
    assert_eq!(git_head(&d.join("a/b")), Some(GitHead::Branch("feat/x".into())));
    fs::write(d.join(".git/HEAD"), "0123456789abcdef\n").unwrap();
    assert_eq!(git_head(&d), Some(GitHead::Detached("0123456".into())));
    let _ = fs::remove_dir_all(&d);
  }

  #[test]
  fn context_from_env() {
    let env = vec![
      ("VIRTUAL_ENV".to_string(), "/x/.venv-foo".to_string()),
      ("HOME".to_string(), "/home/me".to_string()),
      ("SSH_TTY".to_string(), "/dev/pts/1".to_string()),
    ];
    let c = PromptContext::capture(Path::new("/"), &env, Some(2), None, None);
    assert_eq!(c.venv.as_deref(), Some(".venv-foo"));
    assert_eq!(c.home.as_deref(), Some(Path::new("/home/me")));
    assert!(c.ssh);
    assert_eq!(c.last_exit, Some(2));
  }

  fn commit(repo: &Repository, file: &str, msg: &str) -> git2::Oid {
    let root = repo.workdir().unwrap();
    fs::write(root.join(file), msg).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(file)).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("t", "t@example.com").unwrap();
    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    let parents: Vec<_> = parent.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents).unwrap()
  }

  #[test]
  fn status_matches_git_info() {
    let d = temp("status");
    let mut repo = Repository::init(&d).unwrap();
    let first = commit(&repo, "a.txt", "one");
    assert_eq!(compute_status(&d), Some(GitStatus::default()));

    // Untracked files don't count.
    fs::write(d.join("new.txt"), "x").unwrap();
    assert_eq!(compute_status(&d), Some(GitStatus::default()));

    fs::write(d.join("a.txt"), "changed").unwrap();
    let s = compute_status(&d).unwrap();
    assert!(s.unindexed && !s.indexed);

    let mut index = repo.index().unwrap();
    index.add_path(Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let s = compute_status(&d).unwrap();
    assert!(!s.unindexed && s.indexed);

    fs::write(d.join("a.txt"), "changed again").unwrap();
    let s = compute_status(&d).unwrap();
    assert!(s.unindexed && s.indexed);

    let sig = git2::Signature::now("t", "t@example.com").unwrap();
    repo.stash_save(&sig, "wip", None).unwrap();
    let s = compute_status(&d).unwrap();
    assert!(s.stashed && !s.unindexed && !s.indexed);

    // Upstream: one commit behind, then ahead.
    let second = commit(&repo, "b.txt", "two");
    let branch = repo.head().unwrap().shorthand().unwrap().to_string();
    repo.reference("refs/remotes/origin/up", first, true, "").unwrap();
    let mut config = repo.config().unwrap();
    config.set_str(&format!("branch.{branch}.remote"), "origin").unwrap();
    config.set_str(&format!("branch.{branch}.merge"), "refs/heads/up").unwrap();
    config.set_str("remote.origin.url", "/nowhere").unwrap();
    config.set_str("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*").unwrap();
    let s = compute_status(&d).unwrap();
    assert!(s.ahead && !s.behind);
    repo.reference("refs/remotes/origin/up", second, true, "").unwrap();
    let s = compute_status(&d).unwrap();
    assert!(!s.ahead && !s.behind);

    fs::write(d.join(".git/MERGE_HEAD"), format!("{first}\n")).unwrap();
    assert_eq!(compute_status(&d).unwrap().action.as_deref(), Some("MERGE"));
    let _ = fs::remove_dir_all(&d);
  }

  #[test]
  fn action_names() {
    assert_eq!(action_name(RepositoryState::Clean), None);
    assert_eq!(action_name(RepositoryState::RebaseInteractive), Some("REBASE-I"));
    assert_eq!(action_name(RepositoryState::CherryPickSequence), Some("CHERRY-SEQ"));
  }
}
