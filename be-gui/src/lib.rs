mod render;

use be_input::Key;
use kurbo::{Axis, Cap, Line, Rect, Stroke};
pub use render::*;

use crate::{editor::Editor, shell::Shell, theme::Theme};

mod editor;
mod file_tree;
mod shell;
mod theme;

struct State {
  keys:   Vec<Key>,
  active: usize,
  tabs:   Vec<Tab>,
}

struct Tab {
  title:   String,
  content: TabContent,
}

enum TabContent {
  Shell(Shell),
  Editor(Editor),
}

impl State {
  fn draw(&self, render: &mut Render) {
    render.split(
      Axis::Horizontal,
      -20.0,
      |render| match &self.tabs[self.active].content {
        TabContent::Shell(shell) => shell.draw(render),
        TabContent::Editor(editor) => editor.draw(render),
      },
      |render| self.draw_tabs(render),
    );
  }

  fn on_key(&mut self, key: Key) {
    self.keys.push(key);

    match &mut self.tabs[self.active].content {
      TabContent::Shell(_) => {}
      TabContent::Editor(editor) => match editor.on_key(&self.keys) {
        Ok(()) | Err(be_input::ActionError::Unrecognized) => self.keys.clear(),
        Err(be_input::ActionError::Incomplete) => {}
      },
    }
  }

  fn draw_tabs(&self, render: &mut Render) {
    let theme = Theme::current();

    render.fill(
      &Rect::new(0.0, render.size().height - 20.0, render.size().width, render.size().height),
      theme.background_lower,
    );

    let mut x = 10.0;
    for (i, tab) in self.tabs.iter().enumerate() {
      let layout = render.layout_text(&tab.title, (x, render.size().height - 20.0), theme.text);

      if i == self.active {
        render.fill(
          &Rect::new(
            layout.bounds().x0 - 5.0,
            render.size().height - 20.0,
            layout.bounds().x1 + 5.0,
            render.size().height,
          ),
          theme.background,
        );
      }

      x += layout.bounds().width();
      render.draw_text(&layout);

      x += 5.0;
      render.stroke(
        &Line::new((x, render.size().height - 20.0), (x, render.size().height)),
        theme.text,
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
        Tab { title: "zsh".into(), content: TabContent::Shell(Shell::new()) },
        Tab { title: "editor".into(), content: TabContent::Editor(Editor::new()) },
        Tab { title: "zsh".into(), content: TabContent::Shell(Shell::new()) },
      ],
    }
  }
}
