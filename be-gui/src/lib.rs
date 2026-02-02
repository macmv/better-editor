mod render;

use std::{collections::HashMap, hash::Hash};

use be_input::{Action, KeyStroke, Navigation};
use kurbo::{Axis, Cap, Line, Point, Rect, Stroke};
pub use render::*;

use pane::Pane;
use smol_str::SmolStr;
use view::View;

use crate::view::{FileTree, ViewContent};

mod icon;
mod layout;
mod pane;
mod theme;
mod view;
mod widget;

pub use layout::Layout;
pub use widget::{Widget, WidgetStore};

struct State {
  keys:   Vec<KeyStroke>,
  active: usize,
  tabs:   Vec<Tab>,

  views:   ViewCollection,
  widgets: WidgetCollection,

  notify: Notify,
}

struct ViewCollection {
  next_view_id: ViewId,
  views:        HashMap<ViewId, View>,
}

struct WidgetCollection {
  next_widget_id: WidgetId,
  paths:          HashMap<WidgetPath, WidgetId>,
  widgets:        HashMap<WidgetId, WidgetStore>,
}

struct Tab {
  title:   String,
  content: Pane,
  search:  Option<View>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct ViewId(u64);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct WidgetId(u64);

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct WidgetPath(Vec<SmolStr>);

impl State {
  pub fn new(store: &RenderStore) -> Self {
    let mut state = State {
      keys:    vec![],
      active:  1,
      tabs:    vec![],
      views:   ViewCollection::new(),
      widgets: WidgetCollection::new(),
      notify:  store.notifier(),
    };

    let layout = store.config.borrow().settings.layout.clone();

    fn build_view(state: &mut State, store: &RenderStore, tab: be_config::TabSettings) -> Pane {
      match tab {
        be_config::TabSettings::Split(split) => Pane::Split(pane::Split {
          axis:    match split.axis {
            be_config::Axis::Vertical => Axis::Vertical,
            be_config::Axis::Horizontal => Axis::Horizontal,
          },
          percent: split.percent,
          active:  split.active,
          items:   split.children.into_iter().map(|c| build_view(state, store, c)).collect(),
        }),
        be_config::TabSettings::Terminal => {
          Pane::View(state.views.new_view(view::TerminalView::new()))
        }
        be_config::TabSettings::Editor => {
          Pane::View(state.views.new_view(view::EditorView::new(store)))
        }
        be_config::TabSettings::FileTree => {
          Pane::View(state.views.new_view(view::FileTree::current_directory(store.notifier())))
        }
      }
    }

    fn tab_title(tab: &be_config::TabSettings) -> Option<String> {
      match tab {
        be_config::TabSettings::Split(split) => split.children.iter().find_map(|t| tab_title(t)),
        be_config::TabSettings::Terminal => Some("terminal".into()),
        be_config::TabSettings::Editor => Some("editor".into()),
        be_config::TabSettings::FileTree => None,
      }
    }

    for tab in layout.tab {
      let title = tab_title(&tab);
      let view = build_view(&mut state, store, tab);
      state.tabs.push(Tab { title: title.unwrap_or_default(), content: view, search: None });
    }

    for view in state.tabs[state.active].content.views() {
      state.views.get_mut(view).unwrap().on_visible(true);
    }

    state.active_view_mut().on_focus(true);

    state
  }

  fn layout(&mut self, layout: &mut Layout) {
    layout.widgets = Some(std::mem::replace(&mut self.widgets, WidgetCollection::new()));

    layout.split(
      self,
      Axis::Horizontal,
      Distance::Pixels(-20.0),
      "main",
      "tabs",
      |state, layout| {
        let tab = &mut state.tabs[state.active];
        if let Some(search) = &mut tab.search {
          search.layout(layout);
        }
        tab.content.layout(&mut state.views.views, layout);
        if !layout.to_close.is_empty() {
          for to_close in layout.to_close.drain(..) {
            tab.content.close(to_close);
          }

          // Re-run layout after removing closed views.
          tab.content.layout(&mut state.views.views, layout);
        }
      },
      |state, layout| {
        state.layout_tabs(layout);
      },
    );

    self.widgets = layout.widgets.take().unwrap();

    self.widgets.widgets.retain(|id, widget| {
      if !layout.seen.contains(id) {
        self.widgets.paths.remove(&widget.path);
        false
      } else {
        true
      }
    });

    // for widget in self.widgets.values_mut() {
    //   widget.layout(layout);
    // }
  }

  fn animated(&self) -> bool { self.tabs[self.active].content.animated(&self.views.views) }

  fn draw(&mut self, render: &mut Render) {
    render.split(
      self,
      Axis::Horizontal,
      Distance::Pixels(-20.0),
      |state, render| {
        let tab = &mut state.tabs[state.active];
        for view in tab.content.views() {
          let view = state.views.get_mut(view).unwrap();
          render.clipped(view.bounds, |render| view.draw(render));
        }
        if let Some(search) = &mut tab.search {
          render.clipped(
            Rect::new(100.0, 50.0, render.size().width - 100.0, render.size().height - 50.0),
            |render| search.draw(render),
          );
        }
      },
      |state, render| state.draw_tabs(render),
    );

    for widget in self.widgets.values_mut() {
      widget.draw(render);
    }
  }

  fn open(&mut self, path: &std::path::Path) {
    if let ViewContent::Editor(e) = &mut self.active_view_mut().content {
      let _ = e.editor.open(path);
      if let Some(tree) = self.current_file_tree_mut() {
        tree.open(path);
      }
    } else if let Some(e) =
      self.views.views.values_mut().filter(|v| v.visible()).find_map(|v| match &mut v.content {
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

      self.views.get_mut(prev_focus).unwrap().on_focus(false);
      self.views.get_mut(new_focus).unwrap().on_focus(true);

      if let Some(tree) = self.current_file_tree_mut() {
        tree.open(path);
      }
    }
  }

  fn current_file_tree_mut(&mut self) -> Option<&mut FileTree> {
    self.views.visible_mut().find_map(|v| match &mut v.content {
      ViewContent::FileTree(e) => Some(e),
      _ => None,
    })
  }

  fn active_view(&self) -> &View {
    if let Some(search) = &self.tabs[self.active].search {
      search
    } else {
      self.views.get(self.tabs[self.active].content.active()).unwrap()
    }
  }
  fn active_view_mut(&mut self) -> &mut View {
    if self.tabs[self.active].search.is_some() {
      self.tabs[self.active].search.as_mut().unwrap()
    } else {
      self.views.get_mut(self.tabs[self.active].content.active()).unwrap()
    }
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
        self.views.get_mut(self.tabs[prev_active].content.active()).unwrap().on_focus(false);

        for view in self.tabs[prev_active].content.views() {
          self.views.get_mut(view).unwrap().on_visible(false);
        }
        for view in self.tabs[new_active].content.views() {
          self.views.get_mut(view).unwrap().on_visible(true);
        }

        self.views.get_mut(self.tabs[prev_active].content.active()).unwrap().on_focus(true);
      }
      Action::Navigate { nav: Navigation::Direction(dir) } => {
        let prev_focus = self.active_tab().content.active();
        if let Some(new_focus) = self.active_tab_mut().content.focus(dir) {
          self.views.get_mut(prev_focus).unwrap().on_focus(false);
          self.views.get_mut(new_focus).unwrap().on_focus(true);
        }
      }
      Action::Navigate { nav: Navigation::OpenSearch } => {
        self.active_tab_mut().search = Some(View {
          // TODO: Get the window size in here.
          bounds:  Rect::new(0.0, 0.0, 1.0, 1.0),
          content: ViewContent::Search(view::Search::new(self.notify.clone())),
        });
      }
      Action::SetMode { mode: be_input::Mode::Normal, .. }
        if self.active_tab().search.is_some() =>
      {
        self.active_tab_mut().search = None;
      }
      _ => self.active_view_mut().perform_action(action),
    }
  }

  fn layout_tabs(&self, layout: &mut Layout) {
    for (i, tab) in self.tabs.iter().enumerate() {
      layout.add_widget(smol_str::format_smolstr!("tab-{}", i), || {
        crate::widget::Button::new(&tab.title)
      });
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

  fn active_editor(&mut self) -> Option<&mut be_editor::EditorState> {
    if let ViewContent::Editor(e) = &mut self.active_view_mut().content {
      Some(&mut e.editor)
    } else {
      None
    }
  }

  /// Handles an event. Returns `true` if the app should close.
  fn on_event(&mut self, event: Event) -> bool {
    match event {
      Event::Refresh => {}
      Event::OpenFile(path) => {
        self.tabs[self.active].search = None;
        self.open(&path);
      }
      Event::RunCommand(cmd) => {
        let (cmd, args) = cmd.split_once(' ').unwrap_or((&cmd, ""));

        match cmd {
          "w" => {
            if let Some(editor) = self.active_editor() {
              editor.begin_save();
            }
          }
          "q" => return true,
          "e" => {
            if let Some(editor) = self.active_editor() {
              let _ = editor.open(std::path::Path::new(args));
              /*
              .map(|()| format!("{}: opened",
              self.file.as_ref().unwrap().path().display()));
              */
            }
          }
          "noh" => {
            if let Some(editor) = self.active_editor() {
              editor.clear_search();
            }
          }
          "vs" => self.tabs[self.active].content.split(Axis::Vertical, &mut self.views),
          "hs" => self.tabs[self.active].content.split(Axis::Horizontal, &mut self.views),

          _ => {
            println!("unknown command: {}", cmd);
          }
        }
      }
      Event::Exit => return true,
    }

    false
  }
}

impl ViewCollection {
  pub fn new() -> Self { ViewCollection { next_view_id: ViewId(0), views: HashMap::new() } }

  pub fn get(&self, id: ViewId) -> Option<&View> { self.views.get(&id) }
  pub fn get_mut(&mut self, id: ViewId) -> Option<&mut View> { self.views.get_mut(&id) }

  pub fn new_view(&mut self, view: impl Into<View>) -> ViewId {
    let id = self.next_view_id;
    self.next_view_id.0 += 1;
    self.views.insert(id, view.into());
    id
  }

  pub fn visible_mut(&mut self) -> impl Iterator<Item = &mut View> {
    self.views.values_mut().filter(|v| v.visible())
  }
}

impl WidgetCollection {
  pub fn new() -> Self {
    WidgetCollection {
      next_widget_id: WidgetId(0),
      paths:          HashMap::new(),
      widgets:        HashMap::new(),
    }
  }

  pub fn get_path(&self, path: &WidgetPath) -> Option<WidgetId> { self.paths.get(path).copied() }

  pub fn create(&mut self, store: WidgetStore) -> WidgetId {
    let id = self.next_widget_id;
    self.next_widget_id.0 += 1;
    self.paths.insert(store.path.clone(), id);
    self.widgets.insert(id, store);
    id
  }

  pub fn values_mut(&mut self) -> impl Iterator<Item = &mut WidgetStore> {
    self.widgets.values_mut()
  }
}
