mod render;

use be_input::{Action, KeyStroke, Navigation};
use kurbo::{Axis, Cap, Line, Point, Rect, Stroke};
pub use render::*;

use crate::pane::Pane;

mod pane;
mod shell;
mod theme;

struct State {
  keys:   Vec<KeyStroke>,
  active: usize,
  tabs:   Vec<Tab>,
}

struct Tab {
  title:   String,
  content: Pane,
}

impl State {
  fn draw(&mut self, render: &mut Render) {
    render.split(
      self,
      Axis::Horizontal,
      Distance::Pixels(-20.0),
      |state, render| state.tabs[state.active].content.draw(render),
      |state, render| state.draw_tabs(render),
    );
  }

  fn open(&mut self, path: &std::path::Path) { self.tabs[self.active].content.open(path) }

  fn on_key(&mut self, key: KeyStroke) {
    self.keys.push(key);

    match Action::from_input(self.active_tab().content.active().mode(), &self.keys) {
      Ok(action) => {
        self.perform_action(action);
        self.keys.clear();
      }
      Err(be_input::ActionError::Unrecognized) => self.keys.clear(),
      Err(be_input::ActionError::Incomplete) => {}
    }
  }

  fn active_tab(&self) -> &Tab { &self.tabs[self.active] }
  fn active_tab_mut(&mut self) -> &mut Tab { &mut self.tabs[self.active] }

  fn perform_action(&mut self, action: Action) {
    match action {
      Action::Navigate { nav: Navigation::Tab(i) } => {
        self.active = (i as usize).clamp(0, self.tabs.len() - 1)
      }
      _ => self.active_tab_mut().content.perform_action(action),
    }
  }

  fn draw_tabs(&self, render: &mut Render) {
    render
      .fill(&Rect::from_origin_size(Point::ZERO, render.size()), render.theme().background_lower);

    let mut x = 10.0;
    for (i, tab) in self.tabs.iter().enumerate() {
      let layout = render.layout_text(&tab.title, render.theme().text);

      if i == self.active {
        render.fill(
          &Rect::new(
            x - 5.0,
            render.size().height - 20.0,
            x + layout.size().width + 5.0,
            render.size().height,
          ),
          render.theme().background,
        );
      }

      render.draw_text(&layout, (x, 0.0));
      x += layout.size().width;

      x += 5.0;
      render.stroke(
        &Line::new((x, 0.0), (x, render.size().height)),
        render.theme().text,
        Stroke::new(1.0).with_caps(Cap::Butt),
      );
      x += 6.0;
    }
  }
}

impl Default for State {
  fn default() -> Self {
    Self {
      keys:   vec![],
      active: 1,
      tabs:   vec![
        Tab { title: "zsh".into(), content: Pane::new_shell() },
        Tab { title: "editor".into(), content: Pane::new_editor() },
        Tab { title: "zsh".into(), content: Pane::new_shell() },
      ],
    }
  }
}
