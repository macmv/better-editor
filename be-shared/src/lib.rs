//! A shared data structure. This is quite similar to `Rc<RefCell<T>>`, except
//! it has one big caveat: it is unsafe, and has no `borrow` or `borrow_mut`!
//!
//! This type allows you to dereference a shared handle mutably to access the
//! shared state seemlessly:
//! ```
//! let mut handle = SharedHandle::new(3);
//! *handle += 1;
//! assert_eq!(*handle, 4);
//! ```
//!
//! And of course, this is unsafe, as you can clone a handle, and create
//! multiple shared mutable references at the same time.
//!
//! However, it is safe in one major situation: there is only a single clone of
//! a handle in scope. In this project, that is almost always the case. This
//! type is used for things like shared editor state. There is a handle on every
//! view into the same editor. But, no views have access to each other, and so
//! they effectively have exclusive access to the handle for the duration of
//! their methods.
//!
//! In general, I use this type like so:
//! ```
//! struct State { /* ... */ }
//! struct View {
//!   handle: SharedHandle<State>,
//! }
//!
//! impl View {
//!   pub fn update(&mut self) {
//!     // exclusive access to that `State` in `handle`!
//!   }
//! }
//! ```
//!
//! This ensures that the type is safe, if used in this way.

use std::{
  cell::UnsafeCell,
  ops::{Deref, DerefMut},
  rc::{Rc, Weak},
};

/// See the [module level documentation](..) for more information.
pub struct SharedHandle<T> {
  inner: Rc<UnsafeCell<T>>,
}

/// See the [module level documentation](..) for more information.
pub struct WeakHandle<T> {
  inner: Weak<UnsafeCell<T>>,
}

impl<T> From<T> for SharedHandle<T> {
  fn from(value: T) -> Self { SharedHandle::new(value) }
}

impl<T: Default> Default for SharedHandle<T> {
  fn default() -> Self { SharedHandle::new(Default::default()) }
}

impl<T> SharedHandle<T> {
  pub fn new(value: T) -> Self { SharedHandle { inner: Rc::new(UnsafeCell::new(value)) } }

  pub fn downgrade(handle: &Self) -> WeakHandle<T> {
    WeakHandle { inner: Rc::downgrade(&handle.inner) }
  }
}

impl<T> WeakHandle<T> {
  pub fn can_upgrade(&self) -> bool { self.inner.strong_count() > 0 }

  pub fn upgrade(&self) -> Option<SharedHandle<T>> {
    self.inner.upgrade().map(|inner| SharedHandle { inner })
  }
}

impl<T> Deref for SharedHandle<T> {
  type Target = T;

  fn deref(&self) -> &T { unsafe { &*self.inner.get() } }
}

impl<T> DerefMut for SharedHandle<T> {
  fn deref_mut(&mut self) -> &mut T { unsafe { &mut *self.inner.get() } }
}

impl<T> Clone for SharedHandle<T> {
  fn clone(&self) -> Self { Self { inner: self.inner.clone() } }
}
