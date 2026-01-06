use std::{cell::RefCell, time::Instant};

pub struct Animation {
  duration: f64,
  ease:     Ease,
  state:    RefCell<State>,
}

pub enum Ease {
  Linear,
  CubicIn,
  CubicOut,
  CubicInOut,
}

impl Ease {
  pub fn apply(&self, t: f64) -> f64 {
    match self {
      Ease::Linear => t,
      Ease::CubicIn => t * t * t,
      Ease::CubicOut => 1.0 - (1.0 - t).powi(3),
      Ease::CubicInOut => {
        if t < 0.5 {
          4.0 * t * t * t
        } else {
          1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
        }
      }
    }
  }
}

#[derive(Default)]
struct State {
  running:   Option<Instant>,
  direction: Direction,
  time:      f64,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Direction {
  #[default]
  Forward,
  Reverse,
}

macro_rules! ease {
  ($ease:ident, $func:ident) => {
    pub fn $func(duration: f64) -> Animation {
      Animation { duration, state: Default::default(), ease: Ease::$ease }
    }
  };
}

impl Animation {
  ease!(Linear, linear);
  ease!(CubicIn, ease_in);
  ease!(CubicOut, ease_out);
  ease!(CubicInOut, ease_in_out);

  pub fn is_running(&self) -> bool { self.state.borrow().running.is_some() }

  pub fn interpolate(&self, start: f64, end: f64) -> f64 {
    let t = self.state.borrow().time / self.duration;
    start + (end - start) * self.ease.apply(t)
  }

  pub fn run(&mut self, forward: bool) {
    let state = self.state.get_mut();
    state.running = Some(Instant::now());
    state.direction = if forward { Direction::Forward } else { Direction::Reverse };
  }

  pub fn start(&mut self) {
    let state = self.state.get_mut();
    state.running = Some(Instant::now());
    state.time = 0.0;
    state.direction = Direction::Forward;
  }

  pub fn start_reverse(&mut self) {
    let state = self.state.get_mut();
    state.running = Some(Instant::now());
    state.time = self.duration;
    state.direction = Direction::Reverse;
  }

  pub fn stop(&mut self) {
    let state = self.state.get_mut();
    state.running = None;
    if state.direction == Direction::Forward {
      state.time = self.duration;
    } else {
      state.time = 0.0;
    }
  }

  pub fn advance(&self, now: std::time::Instant) {
    let mut state = self.state.borrow_mut();

    let Some(start) = state.running else { return };
    let dt = now - start;
    state.running = Some(now);

    match state.direction {
      Direction::Forward => {
        state.time += dt.as_secs_f64();

        if state.time >= self.duration {
          state.time = self.duration;
          state.running = None;
        }
      }
      Direction::Reverse => {
        state.time -= dt.as_secs_f64();

        if state.time <= 0.0 {
          state.time = 0.0;
          state.running = None;
        }
      }
    }
  }
}
