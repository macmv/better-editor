use std::sync::{Arc, Mutex};

use crate::{Color, oklch};

pub struct Theme {
  pub text:              Color,
  pub background_raised: Color,
  pub background:        Color,
  pub background_lower:  Color,
}

const CURRENT: Mutex<Option<Arc<Theme>>> = Mutex::new(None);

impl Theme {
  fn default_theme() -> Theme {
    Theme {
      text:              oklch(1.0, 0.0, 0.0),
      background_raised: oklch(0.28, 0.03, 288.0),
      background:        oklch(0.23, 0.03, 288.0),
      background_lower:  oklch(0.20, 0.03, 288.0),
    }
  }

  pub fn current() -> Arc<Theme> {
    CURRENT.lock().unwrap().get_or_insert_with(|| Arc::new(Theme::default_theme())).clone()
  }
}
