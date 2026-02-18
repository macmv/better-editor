mod render;

use std::{collections::HashMap, hash::Hash};

use be_doc::Cursor;
use be_input::{Action, KeyStroke, Navigation};
use kurbo::{Axis, Point, Rect, Size};
pub use render::*;

use pane::Pane;
use view::View;

use crate::{
  view::{FileTree, ViewContent},
  widget::{Align, Justify, WidgetCollection},
};

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

  views: ViewCollection,

  tab_layout: WidgetCollection,

  notify: Notify,

  current_hover: Option<ViewId>,
}

struct ViewCollection {
  next_view_id: ViewId,
  views:        HashMap<ViewId, View>,
}

struct Tab {
  title:   String,
  content: Pane,
  popup:   Option<view::Popup>,
}

#[derive(Debug)]
pub enum MouseEvent {
  Move {
    pos: Point,
  },
  Enter,
  Leave,

  /// A button press occurred at the given position.
  ///
  /// Note that the `pos` will always be the same as the previous `Move` event.
  /// It is simply passed for convenience.
  Button {
    pos:     Point,
    pressed: bool,
    button:  MouseButton,
  },
  /// A scroll wheel event occurred at the given position.
  ///
  /// Note that the `pos` will always be the same as the previous `Move` event.
  /// It is simply passed for convenience.
  Scroll {
    pos:   Point,
    delta: kurbo::Vec2,
  },
}

#[derive(Clone, Copy, Debug)]
pub enum MouseButton {
  Left,
  Middle,
  Right,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct ViewId(u64);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct WidgetId(u64);

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct WidgetPath(Vec<u32>);

impl ViewId {
  pub const TABS: ViewId = ViewId(u64::MAX);
}

impl State {
  pub fn new(store: &RenderStore) -> Self {
    let mut state = State {
      keys:          vec![],
      active:        1,
      tabs:          vec![],
      views:         ViewCollection::new(),
      tab_layout:    WidgetCollection::new(),
      notify:        store.notifier(),
      current_hover: None,
    };

    let layout = store.config.borrow().settings.layout.clone();

    fn build_view(state: &mut State, store: &RenderStore, tab: be_config::TabSettings) -> Pane {
      match tab {
        be_config::TabSettings::Split(split) => {
          if split.percent.len() != split.children.len().saturating_sub(1) {
            eprintln!("invalid split percentages"); // TODO: Real errors!
            return Pane::View(state.views.new_view(view::EditorView::new(store)));
          }

          Pane::Split(pane::Split {
            axis:   match split.axis {
              be_config::Axis::Vertical => Axis::Vertical,
              be_config::Axis::Horizontal => Axis::Horizontal,
            },
            active: split.active,
            items:  (split
              .percent
              .iter()
              .copied()
              .chain(std::iter::once(1.0 - split.percent.iter().sum::<f64>()))
              .zip(split.children.into_iter()))
            .map(|(percent, c)| (percent, build_view(state, store, c)))
            .collect(),
          })
        }
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
      state.tabs.push(Tab { title: title.unwrap_or_default(), content: view, popup: None });
    }

    for view in state.tabs[state.active].content.views() {
      state.views.get_mut(view).unwrap().on_visible(true);
    }

    state.active_view_mut().on_focus(true);

    state
  }

  fn layout(&mut self, layout: &mut Layout) {
    layout.clipped(
      Rect::new(0.0, layout.size().height - 25.0, layout.size().width, layout.size().height),
      |layout| {
        self.layout_tabs(layout);
      },
    );

    layout.clipped(
      Rect::new(0.0, 0.0, layout.size().width, layout.size().height - 25.0),
      |layout| {
        let mut tab = &mut self.tabs[self.active];
        if let Some(popup) = &mut tab.popup {
          popup.layout(layout);
        }
        tab.content.layout(&mut self.views.views, layout);
        if !layout.to_close.is_empty() {
          for to_close in layout.to_close.drain(..) {
            if matches!(tab.content, Pane::View(v) if v == to_close) {
              self.tabs.remove(self.active);
              self.active -= 1;
              tab = &mut self.tabs[self.active];
            } else {
              tab.content.close(to_close, &mut self.views.views);
            }
          }

          // Re-run layout after removing closed views.
          tab.content.layout(&mut self.views.views, layout);
        }
      },
    );
  }

  fn hit_view(&self, pos: Point, size: kurbo::Size) -> Option<ViewId> {
    if pos.y < size.height - 25.0 {
      let tab = &self.tabs[self.active];
      tab.content.hit_view(pos, Size::new(size.width, size.height - 25.0))
    } else {
      Some(ViewId::TABS)
    }
  }

  fn on_mouse(&mut self, ev: MouseEvent, size: kurbo::Size, store: &RenderStore) -> CursorKind {
    match ev {
      MouseEvent::Move { pos } => {
        let new_view = self.hit_view(pos, size);
        match (self.current_hover, new_view) {
          (Some(old), Some(new)) if old != new => {
            self.send_mouse_event(old, &MouseEvent::Leave, size, store);
            self.send_mouse_event(new, &MouseEvent::Enter, size, store);
          }
          (Some(old), None) => {
            self.send_mouse_event(old, &MouseEvent::Leave, size, store);
          }
          (None, Some(new)) => {
            self.send_mouse_event(new, &MouseEvent::Enter, size, store);
          }
          _ => {}
        }

        self.current_hover = new_view;
      }

      MouseEvent::Leave => {
        if let Some(old) = self.current_hover {
          self.send_mouse_event(old, &MouseEvent::Leave, size, store);
          self.current_hover = None;
        }
      }

      _ => {}
    }

    if let Some(current) = self.current_hover {
      self.send_mouse_event(current, &ev, size, store).unwrap_or(CursorKind::Default)
    } else {
      CursorKind::Default
    }
  }

  fn send_mouse_event(
    &mut self,
    id: ViewId,
    ev: &MouseEvent,
    size: kurbo::Size,
    store: &RenderStore,
  ) -> Option<CursorKind> {
    match id {
      ViewId::TABS => {
        if let Some(ev) = ev.within(&Rect::new(0.0, size.height - 25.0, size.width, size.height)) {
          return Some(self.tab_layout.on_mouse(&ev, size, store));
        }
      }
      _ => {
        let view = self.views.get_mut(id)?;
        if let Some(ev) = ev.within(&view.bounds) {
          return view.on_mouse(&ev, view.bounds.size(), store);
        }
      }
    }

    None
  }

  fn animated(&self) -> bool { self.tabs[self.active].content.animated(&self.views.views) }

  fn draw(&mut self, render: &mut Render) {
    render.split(
      self,
      Axis::Horizontal,
      Distance::Pixels(-25.0),
      |state, render| {
        let tab = &mut state.tabs[state.active];
        for view in tab.content.views() {
          let view = state.views.get_mut(view).unwrap();
          render.clipped(view.bounds, |render| view.draw(render));
        }
        if let Some(popup) = &mut tab.popup {
          render.clipped(popup.bounds(render.size()), |render| popup.draw(render));
        }
      },
      |state, render| state.draw_tabs(render),
    );
  }

  fn open(&mut self, path: &std::path::Path, cursor: Option<Cursor>) {
    if let ViewContent::Editor(e) = &mut self.active_view_mut().content {
      let res = e.editor.open(path);
      if let Some(cursor) = cursor
        && res.is_ok()
      {
        e.editor.move_to(cursor);
      }

      if let Some(tree) = self.current_file_tree_mut() {
        tree.open(path);
      }
    } else if let Some(e) =
      self.views.views.values_mut().filter(|v| v.visible()).find_map(|v| match &mut v.content {
        ViewContent::Editor(e) => Some(e),
        _ => None,
      })
    {
      let res = e.editor.open(path);
      if let Some(cursor) = cursor
        && res.is_ok()
      {
        e.editor.move_to(cursor);
      }

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

  fn mode(&self) -> be_input::Mode {
    if self.tabs[self.active].popup.is_some() {
      be_input::Mode::Insert
    } else {
      self.views.get(self.tabs[self.active].content.active()).unwrap().mode()
    }
  }
  fn active_view_mut(&mut self) -> &mut View {
    self.views.get_mut(self.tabs[self.active].content.active()).unwrap()
  }

  fn on_key(&mut self, key: KeyStroke) {
    self.keys.push(key);

    let temporary_underline = self.keys.len() == 1
      && matches!(self.keys[0].key, be_input::Key::Char('r' | 'c' | 'd'))
      && !self.keys[0].control;
    if let ViewContent::Editor(e) = &mut self.active_view_mut().content {
      e.temporary_underline = temporary_underline;
    }

    match Action::from_input(self.mode(), &self.keys) {
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

        self.views.get_mut(self.tabs[new_active].content.active()).unwrap().on_focus(true);
      }
      Action::Navigate { nav: Navigation::Direction(dir) } => {
        let prev_focus = self.active_tab().content.active();
        if let Some(new_focus) = self.active_tab_mut().content.focus(dir) {
          self.views.get_mut(prev_focus).unwrap().on_focus(false);
          self.views.get_mut(new_focus).unwrap().on_focus(true);
        }
      }
      Action::Navigate { nav: Navigation::OpenSearch } => {
        self.active_tab_mut().popup =
          Some(view::Popup::Search(view::Search::new(self.notify.clone())));
      }
      Action::SetMode { mode: be_input::Mode::Command, .. } => {
        self.active_tab_mut().popup =
          Some(view::Popup::Command(view::CommandView::new(self.notify.clone())));
      }
      Action::SetMode { mode: be_input::Mode::Normal, .. } if self.active_tab().popup.is_some() => {
        self.active_tab_mut().popup = None;
      }
      _ => {
        if let Some(popup) = &mut self.active_tab_mut().popup {
          popup.perform_action(action)
        } else {
          self.active_view_mut().perform_action(action)
        }
      }
    }
  }

  fn layout_tabs(&mut self, layout: &mut Layout) {
    let mut tab_layout = self.tab_layout.begin(layout);

    let mut row = vec![];

    for (i, tab) in self.tabs.iter().enumerate() {
      let button = tab_layout.add_widget(crate::widget::Button::new(&tab.title));

      if button.pressed() {
        self.active = i;
      }

      row.push(button.id);
    }

    let root = tab_layout
      .add_widget(
        crate::widget::Stack::new(Axis::Horizontal, Align::Start, Justify::Center, row).gap(5.0),
      )
      .id;
    tab_layout.finish(root);
  }

  fn draw_tabs(&mut self, render: &mut Render) {
    render
      .fill(&Rect::from_origin_size(Point::ZERO, render.size()), render.theme().background_lower);

    self.tab_layout.draw(render);
  }

  fn active_editor(&mut self) -> Option<&mut be_editor::EditorState> {
    if let ViewContent::Editor(e) = &mut self.active_view_mut().content {
      Some(&mut e.editor)
    } else {
      None
    }
  }

  /// Handles an event. Returns `true` if the app should close.
  fn on_event(&mut self, event: Event, store: &RenderStore) -> bool {
    match event {
      Event::Refresh => {}
      Event::Editor(be_editor::EditorEvent::OpenFile(path, cursor)) => {
        self.tabs[self.active].popup = None;
        self.open(&path, cursor);
      }
      Event::Editor(be_editor::EditorEvent::RunCommand(cmd)) => {
        let (cmd, args) = cmd.split_once(' ').unwrap_or((&cmd, ""));

        match cmd {
          "w" => {
            if let Some(editor) = self.active_editor() {
              editor.begin_save();
            }
          }
          "q" => {
            let tab = &mut self.tabs[self.active];
            tab.content.close(tab.content.active(), &mut self.views.views);
          }
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
          "vs" => self.tabs[self.active].content.split(Axis::Vertical, &mut self.views, store),
          "hs" => self.tabs[self.active].content.split(Axis::Horizontal, &mut self.views, store),

          _ => {
            println!("unknown command: {}", cmd);
          }
        }

        self.tabs[self.active].popup = None;
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

impl MouseEvent {
  pub fn within(&self, bounds: &Rect) -> Option<MouseEvent> {
    match *self {
      MouseEvent::Move { pos } => {
        if bounds.contains(pos) {
          Some(MouseEvent::Move { pos: pos - bounds.origin().to_vec2() })
        } else {
          None
        }
      }
      MouseEvent::Button { pos, pressed, button } => {
        if bounds.contains(pos) {
          Some(MouseEvent::Button { pos: pos - bounds.origin().to_vec2(), pressed, button })
        } else {
          None
        }
      }
      MouseEvent::Scroll { pos, delta } => {
        if bounds.contains(pos) {
          Some(MouseEvent::Scroll { pos: pos - bounds.origin().to_vec2(), delta })
        } else {
          None
        }
      }
      MouseEvent::Enter => Some(MouseEvent::Enter),
      MouseEvent::Leave => Some(MouseEvent::Leave),
    }
  }
}
