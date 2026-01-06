use parking_lot::Mutex;
use std::{
  any::Any,
  sync::{Arc, Weak},
};

#[derive(Clone)]
pub struct Task<T: 'static> {
  inner:    Arc<Mutex<TaskData>>,
  _phantom: std::marker::PhantomData<T>,
}

pub struct Completer<T: 'static> {
  inner:    Weak<Mutex<TaskData>>,
  _phantom: std::marker::PhantomData<T>,
}

struct TaskData {
  complete: bool,
  result:   Option<Box<dyn Any + Send>>,
  mapper:   Option<Box<dyn FnOnce(Box<dyn Any + Send>) -> Box<dyn Any + Send> + Send>>,
}

impl<T: 'static> Task<T> {
  pub fn new() -> Task<T> {
    Task {
      inner:    Arc::new(Mutex::new(TaskData { complete: false, result: None, mapper: None })),
      _phantom: std::marker::PhantomData,
    }
  }

  pub fn completer(&self) -> Completer<T> {
    Completer { inner: Arc::downgrade(&self.inner), _phantom: std::marker::PhantomData }
  }

  pub fn completed(&self) -> Option<T> {
    let mut inner = self.inner.lock();
    let res = inner.result.take()?;

    let res = if let Some(mapper) = inner.mapper.take() { mapper(res) } else { res };
    Some(*res.downcast().expect("task data contained the wrong type"))
  }

  pub fn map<U: Send>(self, f: impl FnOnce(T) -> U + Send + 'static) -> Task<U> {
    {
      let mut inner = self.inner.lock();
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

    Task { inner: self.inner, _phantom: std::marker::PhantomData }
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
