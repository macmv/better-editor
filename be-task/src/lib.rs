use parking_lot::Mutex;
use std::{
  any::Any,
  cell::RefCell,
  sync::{Arc, Weak},
};

/// A result that isn't there yet. Either another thread fills it in through a
/// `Completer`, or the task polls for it itself.
pub enum Task<T: 'static> {
  /// Completed through a `Completer`, from any thread.
  Shared { inner: Arc<Mutex<TaskData>>, _phantom: std::marker::PhantomData<T> },
  /// Polled on the thread that owns it, such as an RPC call that is answered
  /// as its connection is driven. Not `Send`, and can't be cloned.
  Poll(RefCell<PollData<T>>),
}

pub struct Completer<T: 'static> {
  inner:    Weak<Mutex<TaskData>>,
  _phantom: std::marker::PhantomData<T>,
}

pub struct TaskData {
  complete: bool,
  result:   Option<Box<dyn Any + Send>>,
  mapper:   Option<Box<dyn FnOnce(Box<dyn Any + Send>) -> Box<dyn Any + Send> + Send>>,
}

pub struct PollData<T> {
  poll: Box<dyn FnMut() -> Option<T>>,
  done: bool,
}

impl<T: 'static> Clone for Task<T> {
  /// Only a task with a `Completer` can be cloned.
  fn clone(&self) -> Self {
    match self {
      Task::Shared { inner, _phantom } => {
        Task::Shared { inner: inner.clone(), _phantom: *_phantom }
      }
      Task::Poll(_) => panic!("a polled task cannot be cloned"),
    }
  }
}

impl<T: 'static> Task<T> {
  /// A task that a `Completer` finishes.
  pub fn new() -> Task<T> {
    Task::Shared {
      inner:    Arc::new(Mutex::new(TaskData { complete: false, result: None, mapper: None })),
      _phantom: std::marker::PhantomData,
    }
  }

  /// A task that finishes when `poll` returns `Some`. `poll` is not called
  /// again after that.
  pub fn from_poll(poll: impl FnMut() -> Option<T> + 'static) -> Task<T> {
    Task::Poll(RefCell::new(PollData { poll: Box::new(poll), done: false }))
  }

  /// A task that has already finished.
  pub fn ready(value: T) -> Task<T> {
    let mut value = Some(value);
    Task::from_poll(move || value.take())
  }

  /// Panics on a task made with `from_poll` or `ready`, which finish
  /// themselves.
  pub fn completer(&self) -> Completer<T> {
    match self {
      Task::Shared { inner, .. } => {
        Completer { inner: Arc::downgrade(inner), _phantom: std::marker::PhantomData }
      }
      Task::Poll(_) => panic!("a polled task has no completer"),
    }
  }

  /// The result, once there is one. After that, this returns `None` forever.
  pub fn completed(&self) -> Option<T> {
    match self {
      Task::Shared { inner, .. } => {
        let mut inner = inner.lock();
        let res = inner.result.take()?;

        let res = if let Some(mapper) = inner.mapper.take() { mapper(res) } else { res };
        Some(*res.downcast().expect("task data contained the wrong type"))
      }
      Task::Poll(data) => {
        let mut data = data.borrow_mut();
        if data.done {
          return None;
        }
        let res = (data.poll)()?;
        data.done = true;
        Some(res)
      }
    }
  }

  pub fn map<U: Send>(self, f: impl FnOnce(T) -> U + Send + 'static) -> Task<U> {
    match self {
      Task::Shared { inner: data, .. } => {
        {
          let mut inner = data.lock();
          if let Some(mapper) = inner.mapper.take() {
            inner.mapper = Some(Box::new(move |v: Box<dyn Any + Send>| {
              let v = mapper(v);
              let res = f(*v.downcast().expect("task data contained the wrong type"));
              Box::new(res) as Box<dyn Any + Send>
            }));
          } else {
            inner.mapper = Some(Box::new(move |v: Box<dyn Any + Send>| {
              let res = f(*v.downcast().expect("task data contained the wrong type"));
              Box::new(res) as Box<dyn Any + Send>
            }));
          };
        }

        Task::Shared { inner: data, _phantom: std::marker::PhantomData }
      }
      Task::Poll(data) => {
        let mut data = data.into_inner();
        let mut f = Some(f);
        Task::from_poll(move || {
          if data.done {
            return None;
          }
          let v = (data.poll)()?;
          data.done = true;
          Some((f.take()?)(v))
        })
      }
    }
  }
}

impl<T: Send + 'static> Completer<T> {
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
    inner.result = Some(Box::new(result));
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

  #[test]
  fn poll_works() {
    let mut calls = 0;
    let task = Task::from_poll(move || {
      calls += 1;
      (calls == 3).then_some(calls)
    });

    assert_eq!(task.completed(), None);
    assert_eq!(task.completed(), None);
    assert_eq!(task.completed(), Some(3));
    assert_eq!(task.completed(), None);
  }

  #[test]
  fn poll_map_works() {
    let task = Task::ready(21).map(|v| v * 2).map(|v| v + 1);
    assert_eq!(task.completed(), Some(43));
    assert_eq!(task.completed(), None);
  }

  #[test]
  fn map_works() {
    let task = Task::<u32>::new();

    let completer = task.completer();
    std::thread::spawn(move || completer.complete(42).unwrap());

    let t2 = task.map(|v| f64::from(v));

    loop {
      let res = t2.completed();
      if res.is_none() {
        std::thread::sleep(std::time::Duration::from_millis(1));
      } else {
        assert_eq!(res, Some(42.0));
        break;
      }
    }
  }

  #[test]
  fn map_works_twice() {
    let task = Task::<u32>::new();

    let completer = task.completer();
    std::thread::spawn(move || completer.complete(42).unwrap());

    let t2 = task.map(|v| f64::from(v));
    let t3 = t2.map(|v| v as u8);

    loop {
      let res = t3.completed();
      if res.is_none() {
        std::thread::sleep(std::time::Duration::from_millis(1));
      } else {
        assert_eq!(res, Some(42));
        break;
      }
    }
  }
}
