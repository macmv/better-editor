use parking_lot::Mutex;
use std::sync::{Arc, Weak};

#[derive(Clone)]
pub struct Task<T> {
  inner: Arc<Mutex<TaskData<T>>>,
}

pub struct Completer<T> {
  inner: Weak<Mutex<TaskData<T>>>,
}

struct TaskData<T> {
  complete: bool,
  result:   Option<T>,
}

impl<T> Task<T> {
  pub fn new() -> Task<T> {
    Task { inner: Arc::new(Mutex::new(TaskData { complete: false, result: None })) }
  }

  pub fn completer(&self) -> Completer<T> { Completer { inner: Arc::downgrade(&self.inner) } }

  pub fn completed(&self) -> Option<T> { self.inner.lock().result.take() }
}

impl<T> Completer<T> {
  /// This is racy! Only use this to cleanup old `Completer`s that aren't used
  /// elsewhere.
  pub fn is_live(&self) -> bool { self.inner.strong_count() > 0 }

  /// Completes the task. Returns `Err(result)` if the task was dropped or
  /// already completed.
  pub fn complete(self, result: T) -> Result<(), T> {
    let Some(inner) = self.inner.upgrade() else { return Err(result) };
    let mut inner = inner.lock();
    if inner.complete || inner.result.is_some() {
      return Err(result);
    }
    inner.complete = true;
    inner.result = Some(result);
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn it_works() {
    let task = Task::<u32>::new();

    let completer = task.completer();
    std::thread::spawn(move || completer.complete(42).unwrap());

    loop {
      let res = task.completed();
      if res.is_none() {
        std::thread::sleep(std::time::Duration::from_millis(1));
      } else {
        assert_eq!(res, Some(42));
        break;
      }
    }
  }
}
