#[doc(hidden)]
pub use log;

pub trait ResultExt<T, E> {
  fn fatal(self) -> Option<T>;
}

impl<T, E: std::fmt::Display> ResultExt<T, E> for Result<T, E> {
  fn fatal(self) -> Option<T> {
    match self {
      Ok(t) => Some(t),
      Err(e) => {
        fatal!("{e}");
        None
      }
    }
  }
}

#[macro_export]
#[cfg(debug_assertions)]
macro_rules! fatal {
  ($($tt:tt)*) => {
    if true { panic!($($tt)*) }
  };
}

#[macro_export]
#[cfg(not(debug_assertions))]
macro_rules! fatal {
  ($($tt:tt)*) => {
    $crate::log::error!($($tt)*)
  };
}
