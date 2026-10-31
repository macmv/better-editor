use std::{
  cell::RefCell,
  collections::HashMap,
  path::Path,
  time::{Duration, Instant},
};

use be_git::{GitBackend, PathStatus};
use be_task::Task;
use lode_local::Pending;

use crate::{GitFileStatus, Host, HostError, Transport};

/// How often HEAD is asked for again. The editor checks it on every layout, so
/// without this it would be a round trip per frame.
const HEAD_TTL: Duration = Duration::from_millis(500);

/// Git state on the remote machine, for the repository at `root`.
pub struct RemoteGit {
  t:        Transport,
  root:     String,
  head:     RefCell<Head>,
  statuses: RefCell<HashMap<String, Status>>,
}

#[derive(Default)]
struct Head {
  /// The latest answer.
  value:   Option<String>,
  /// When the question behind `pending` or the last answer was asked.
  asked:   Option<Instant>,
  pending: Option<Pending<Option<String>, HostError>>,
}

/// One path's status, from asking to being taken.
enum Status {
  Pending(Pending<GitFileStatus, HostError>),
  Ready(GitFileStatus),
}

impl RemoteGit {
  pub fn new(t: Transport, root: &Path) -> RemoteGit {
    RemoteGit {
      t,
      root: root.to_string_lossy().into_owned(),
      head: RefCell::new(Head::default()),
      statuses: RefCell::new(HashMap::new()),
    }
  }
}

impl GitBackend for RemoteGit {
  fn head(&self) -> Option<String> {
    let mut head = self.head.borrow_mut();

    if let Some(call) = &mut head.pending
      && let Some(res) = call.try_take()
    {
      head.pending = None;
      // A failed question keeps the last answer.
      if let Ok(value) = res {
        head.value = value;
      }
    }

    if head.pending.is_none() && head.asked.is_none_or(|at| at.elapsed() >= HEAD_TTL) {
      head.pending = Some(self.t.local().proxy::<Host>().git_head(self.root.clone()));
      head.asked = Some(Instant::now());
      self.t.io_done();
    }

    head.value.clone()
  }

  fn begin_lookup_in_head(&self, path: &Path) -> Task<Option<String>> {
    let mut call = self
      .t
      .local()
      .proxy::<Host>()
      .git_lookup(self.root.clone(), path.to_string_lossy().into_owned());
    self.t.io_done();
    Task::from_poll(move || Some(call.try_take()?.ok().flatten()))
  }

  fn status(&self, path: &Path) -> Option<PathStatus> {
    let path = path.to_string_lossy().into_owned();
    let mut statuses = self.statuses.borrow_mut();

    if let Some(Status::Pending(call)) = statuses.get_mut(&path)
      && let Some(res) = call.try_take()
    {
      // A failed question counts as a clean path.
      statuses.insert(path.clone(), Status::Ready(res.unwrap_or_default()));
    }

    match statuses.remove(&path) {
      Some(Status::Ready(s)) => Some(PathStatus {
        ignored:  s.ignored,
        added:    s.added,
        modified: s.modified,
      }),
      Some(pending @ Status::Pending(_)) => {
        statuses.insert(path, pending);
        None
      }
      None => {
        let call = self.t.local().proxy::<Host>().git_status(self.root.clone(), path.clone());
        statuses.insert(path, Status::Pending(call));
        self.t.io_done();
        None
      }
    }
  }
}
