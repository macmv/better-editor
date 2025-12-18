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
  fn draw(&self, render: &mut Render) { self.draw_tabs(render); }

  fn draw_tabs(&self, render: &mut Render) {
    render.fill(
      &Rect::new(0.0, render.size().height - 20.0, render.size().width, render.size().height),
      oklch(0.3, 0.0, 0.0),
    );

    let mut x = 10.0;
    for (i, tab) in self.tabs.iter().enumerate() {
      let layout =
        render.layout_text(&tab.title, (x, render.size().height - 20.0), oklch(1.0, 0.0, 0.0));

      if i == self.active {
        render.fill(
          &Rect::new(
            layout.bounds().x0 - 5.0,
            render.size().height - 20.0,
            layout.bounds().x1 + 5.0,
            render.size().height,
          ),
          oklch(0.5, 0.0, 0.0),
        );
      }

      x += layout.bounds().width();
      render.draw_text(layout);

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
