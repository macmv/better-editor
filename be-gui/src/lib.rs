mod render;

use kurbo::{Cap, Line, Rect, Stroke};
pub use render::*;

struct State {
  active: usize,
  tabs:   Vec<Tab>,
}

struct Tab {
  title: String,
}

impl State {
  fn draw(&self, render: &mut Render) {
    render.fill(
      &Rect::new(0.0, render.size().height - 20.0, render.size().width, render.size().height),
      oklch(0.3, 0.0, 0.0),
    );

    let mut x = 0.0;
    for tab in &self.tabs {
      let rect =
        render.draw_text(&tab.title, (x, render.size().height - 20.0), oklch(1.0, 0.0, 0.0));
      x += rect.width();

      x += 5.0;
      render.stroke(
        &Line::new((x, render.size().height - 20.0), (x, render.size().height)),
        oklch(1.0, 0.0, 0.0),
        Stroke::new(1.0).with_caps(Cap::Butt),
      );
      x += 6.0;
    }
  }
}

impl Default for State {
  fn default() -> Self {
    Self {
      active: 1,
      tabs:   vec![
        Tab { title: "zsh".into() },
        Tab { title: "editor".into() },
        Tab { title: "editor".into() },
      ],
    }
  }
}
