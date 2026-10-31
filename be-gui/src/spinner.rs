use std::{f64::consts::TAU, time::Instant};

use kurbo::{Arc, Cap, Point, Stroke, Vec2};

use crate::Render;

/// Diameter of the spinner, in pixels.
const SIZE: f64 = 100.0;
const THICKNESS: f64 = 6.0;
/// How much of the circle the arc covers.
const SWEEP: f64 = TAU * 0.25;
/// Turns per second.
const SPEED: f64 = 0.8;

/// An arc that goes around forever. The caller must keep redrawing while it
/// is on screen.
pub struct Spinner {
  start: Instant,
}

impl Spinner {
  pub fn new() -> Self { Spinner { start: Instant::now() } }

  pub fn draw(&self, render: &mut Render, center: Point) {
    let radius = (SIZE - THICKNESS) / 2.0;
    let angle = (render.now() - self.start).as_secs_f64() * SPEED * TAU;

    let arc = Arc::new(center, Vec2::new(radius, radius), angle, SWEEP, 0.0);
    render.stroke(&arc, render.theme().text, Stroke::new(THICKNESS).with_caps(Cap::Round));
  }
}
