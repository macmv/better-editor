use std::{
  cell::RefCell,
  path::Path,
  time::{Duration, Instant},
};

use be_git::GitBackend;

use crate::{GitFileStatus, Host, Transport};

/// How long the last status answer is reused. The file tree asks for a
/// path's ignored, added, and modified flags back to back; this makes that
/// one round trip instead of three.
const STATUS_TTL: Duration = Duration::from_millis(100);

/// Git state on the remote machine, for the repository at `root`.
pub struct RemoteGit {
  t:    Transport,
  root: String,
  last: RefCell<Option<(String, Instant, GitFileStatus)>>,
}

impl RemoteGit {
  pub fn new(t: Transport, root: &Path) -> RemoteGit {
    RemoteGit { t, root: root.to_string_lossy().into_owned(), last: RefCell::new(None) }
  }

  fn status(&self, path: &Path) -> GitFileStatus {
    let path = path.to_string_lossy().into_owned();
    if let Some((p, at, status)) = &*self.last.borrow()
      && *p == path
      && at.elapsed() < STATUS_TTL
    {
      return *status;
    }

    let call = self.t.local().proxy::<Host>().git_status(self.root.clone(), path.clone());
    let status = self.t.wait(call).unwrap_or_default();
    *self.last.borrow_mut() = Some((path, Instant::now(), status));
    status
  }
}

impl GitBackend for RemoteGit {
  fn head(&self) -> Option<String> {
    let call = self.t.local().proxy::<Host>().git_head(self.root.clone());
    self.t.wait(call).ok().flatten()
  }

  fn lookup_in_head(&self, path: &Path) -> Option<String> {
    let call = self
      .t
      .local()
      .proxy::<Host>()
      .git_lookup(self.root.clone(), path.to_string_lossy().into_owned());
    self.t.wait(call).ok().flatten()
  }

  fn is_ignored(&self, path: &Path) -> bool { self.status(path).ignored }
  fn is_added(&self, path: &Path) -> bool { self.status(path).added }
  fn is_modified(&self, path: &Path) -> bool { self.status(path).modified }
}
