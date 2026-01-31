use std::fmt;

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

#[doc(hidden)]
#[track_caller]
pub fn fatal_impl(args: &fmt::Arguments) {
  #[cfg(debug_assertions)]
  panic!("{}", args);
  #[cfg(not(debug_assertions))]
  log::error!("{}", args);
}

#[macro_export]
macro_rules! fatal {
  ($($tt:tt)*) => {
    $crate::fatal_impl(&format_args!($($tt)*))
  };
}
