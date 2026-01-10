mod render;

use std::{collections::HashMap, hash::Hash};

use be_input::{Action, KeyStroke, Navigation};
use kurbo::{Axis, Cap, Line, Point, Rect, Stroke};
pub use render::*;

use pane::Pane;
use view::View;

use crate::view::ViewContent;

mod pane;
mod theme;
mod view;

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
  search:  Option<view::Search>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct ViewId(u64);

impl State {
  pub fn new(store: &RenderStore) -> Self {
    let mut state = State {
      keys:         vec![],
      active:       1,
      tabs:         vec![],
      next_view_id: ViewId(0),
      views:        HashMap::new(),
    };

    let shell = state.new_view(view::Shell::new());
    state.tabs.push(Tab {
      title:   "zsh".to_owned(),
      content: pane::Pane::View(shell),
      search:  None,
    });

    let file_tree = state.new_view(view::FileTree::current_directory(store.notifier()));
    let editor = state.new_view(view::EditorView::new(store));
    state.tabs.push(Tab {
      title:   "editor".to_owned(),
      content: Pane::Split(pane::Split {
        axis:    Axis::Vertical,
        percent: vec![0.2],
        active:  1,
        items:   vec![Pane::View(file_tree), Pane::View(editor)],
      }),
      search:  None,
    });

    let shell = state.new_view(view::Shell::new());
    state.tabs.push(Tab {
      title:   "zsh".to_owned(),
      content: pane::Pane::View(shell),
      search:  None,
    });

    state
  }

  fn new_view(&mut self, view: impl Into<View>) -> ViewId {
    let id = self.next_view_id;
    self.next_view_id.0 += 1;
    self.views.insert(id, view.into());
    id
  }

  fn layout(&mut self, layout: &mut Layout) {
    layout.split(
      self,
      Axis::Horizontal,
      Distance::Pixels(-20.0),
      |state, layout| state.tabs[state.active].content.layout(&mut state.views, layout),
      |_, _| {},
    );
  }

  fn animated(&self) -> bool { self.tabs[self.active].content.animated(&self.views) }

  fn draw(&mut self, render: &mut Render) {
    render.split(
      self,
      Axis::Horizontal,
      Distance::Pixels(-20.0),
      |state, render| {
        let tab = &mut state.tabs[state.active];
        tab.content.draw(&mut state.views, render);
        if let Some(search) = &mut tab.search {
          render.clipped(
            Rect::new(200.0, 200.0, render.size().width - 200.0, render.size().height - 200.0),
            |render| {
              search.draw(render);
            },
          );
        }
      },
      |state, render| state.draw_tabs(render),
    );
  }

  fn open(&mut self, path: &std::path::Path) {
    if let ViewContent::Editor(e) = &mut self.active_view_mut().content {
      let _ = e.editor.open(path);
    } else if let Some(e) =
      self.views.values_mut().filter(|v| v.visible()).find_map(|v| match &mut v.content {
        ViewContent::Editor(e) => Some(e),
        _ => None,
      })
    {
      let _ = e.editor.open(path);

      let prev_focus = self.active_tab().content.active();
      // FIXME: Need some way of focusing a particular view id.
      match &mut self.active_tab_mut().content {
        Pane::Split(s) => s.active = 1,
        _ => {}
      }

      let new_focus = self.active_tab().content.active();

      self.views.get_mut(&prev_focus).unwrap().on_focus(false);
      self.views.get_mut(&new_focus).unwrap().on_focus(true);
    }
  }

  fn active_view(&self) -> &View { &self.views[&self.tabs[self.active].content.active()] }
  fn active_view_mut(&mut self) -> &mut View {
    self.views.get_mut(&self.tabs[self.active].content.active()).unwrap()
  }

  fn on_key(&mut self, key: KeyStroke) {
    self.keys.push(key);

    let temporary_underline = self.keys.len() == 1
      && matches!(self.keys[0].key, be_input::Key::Char('r' | 'c' | 'd'))
      && !self.keys[0].control;
    if let ViewContent::Editor(e) = &mut self.active_view_mut().content {
      e.temporary_underline = temporary_underline;
    }

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
        let new_index = (i as usize).clamp(0, self.tabs.len() - 1);
        if new_index == self.active {
          return;
        }

        let prev_active = self.active;
        self.active = new_index;
        let new_active = self.active;

        // Ordering: lose focus, lose visibility, gain visibility, gain focus.
        self.views.get_mut(&self.tabs[prev_active].content.active()).unwrap().on_focus(false);

        for view in self.tabs[prev_active].content.views() {
          self.views.get_mut(&view).unwrap().on_visible(false);
        }
        for view in self.tabs[new_active].content.views() {
          self.views.get_mut(&view).unwrap().on_visible(true);
        }

        self.views.get_mut(&self.tabs[prev_active].content.active()).unwrap().on_focus(true);
      }
      Action::Navigate { nav: Navigation::Direction(dir) } => {
        let prev_focus = self.active_tab().content.active();
        if let Some(new_focus) = self.active_tab_mut().content.focus(dir) {
          self.views.get_mut(&prev_focus).unwrap().on_focus(false);
          self.views.get_mut(&new_focus).unwrap().on_focus(true);
        }
      }
      Action::Navigate { nav: Navigation::OpenSearch } => {
        self.active_tab_mut().search = Some(view::Search::new());
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

  fn on_event(&mut self, event: Event) {
    match event {
      Event::Refresh => {}
      Event::OpenFile(path) => self.open(&path),
      Event::Exit => {} // Handled by `window`
    }
  }
}
