mod render;

use std::{cell::RefCell, collections::HashMap, hash::Hash, rc::Rc};

use be_config::Config;
use be_input::{Action, KeyStroke, Navigation};
use kurbo::{Axis, Cap, Line, Point, Rect, Stroke};
pub use render::*;

use crate::pane::{Pane, View};

mod pane;
mod theme;

struct State {
  keys:   Vec<KeyStroke>,
  active: usize,
  tabs:   Vec<Tab>,

  next_view_id: ViewId,
  views:        HashMap<ViewId, View>,
}

struct Tab {
  title:   String,
  content: Pane,
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
struct ViewId(u64);

impl State {
  pub fn new(config: &Rc<RefCell<Config>>) -> Self {
    let mut state = State {
      keys:         vec![],
      active:       1,
      tabs:         vec![],
      next_view_id: ViewId(0),
      views:        HashMap::new(),
    };

    let shell = state.new_view(View::Shell(pane::Shell::new()));
    state.tabs.push(Tab { title: "zsh".to_owned(), content: pane::Pane::View(shell) });

    let file_tree = state.new_view(View::FileTree(pane::FileTree::current_directory()));
    let editor = state.new_view(View::Editor(pane::EditorView::new(config)));
    state.tabs.push(Tab {
      title:   "editor".to_owned(),
      content: Pane::Split(pane::Split {
        axis:    Axis::Vertical,
        percent: vec![0.2],
        active:  1,
        items:   vec![Pane::View(file_tree), Pane::View(editor)],
      }),
    });

    let shell = state.new_view(View::Shell(pane::Shell::new()));
    state.tabs.push(Tab { title: "zsh".to_owned(), content: pane::Pane::View(shell) });

    state
  }

  fn new_view(&mut self, view: View) -> ViewId {
    let id = self.next_view_id;
    self.next_view_id.0 += 1;
    self.views.insert(id, view);
    id
  }

  fn draw(&mut self, render: &mut Render) {
    render.split(
      self,
      Axis::Horizontal,
      Distance::Pixels(-20.0),
      |state, render| state.tabs[state.active].content.draw(&mut state.views, render),
      |state, render| state.draw_tabs(render),
    );
  }

  fn open(&mut self, path: &std::path::Path) {
    if let View::Editor(e) = self.active_view_mut() {
      let _ = e.editor.open(path);
    }
  }

  fn active_view(&self) -> &View { &self.views[&self.tabs[self.active].content.active()] }
  fn active_view_mut(&mut self) -> &mut View {
    self.views.get_mut(&self.tabs[self.active].content.active()).unwrap()
  }

  fn on_key(&mut self, key: KeyStroke) {
    self.keys.push(key);

    match Action::from_input(self.active_view().mode(), &self.keys) {
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
      Action::Navigate { nav: Navigation::Direction(dir) } => {
        let prev_focus = self.active_tab().content.active();
        if let Some(new_focus) = self.active_tab_mut().content.focus(dir) {
          self.views.get_mut(&prev_focus).unwrap().on_focus(false);
          self.views.get_mut(&new_focus).unwrap().on_focus(true);
        }
      }
      _ => self.active_view_mut().perform_action(action),
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
