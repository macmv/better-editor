use std::fmt;

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
