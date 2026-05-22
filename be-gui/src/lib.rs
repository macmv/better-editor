mod render;

use std::{
  collections::HashMap,
  hash::Hash,
  ops::{Index, IndexMut},
  path::PathBuf,
};

use be_editor::CursorOrLsp;
use be_fs::WorkspaceRoot;
use be_input::{Action, KeyStroke, Mode, Navigation};
use be_workspace::{Workspace, WorkspaceEvent};
use kurbo::{Axis, Point, Rect, Size};
pub use render::*;

use pane::{HitResult, Pane, SplitPath};
use view::View;

use crate::{
  view::{EditorView, FileTree, ViewContent},
  widget::{Align, Justify, WidgetCollection},
};

mod icon;
mod layout;
mod pane;
mod theme;
mod view;
mod widget;

pub use layout::{Font, Layout};
pub use widget::{Widget, WidgetStore};

#[macro_use]
extern crate log;

#[macro_use]
extern crate be_macros;

#[derive(Default, clap::Parser)]
pub struct Args {
  #[clap(long)]
  editor: Option<PathBuf>,
}

struct State {
  keys: Vec<KeyStroke>,

  terminal:             crate::view::TerminalView,
  terminal_should_exit: bool,

  workspaces: HashMap<WorkspaceRoot, WorkspaceLayout>,

  active: AppView,
}

enum AppView {
  Terminal,
  Workspace(WorkspaceRoot),
}

struct WorkspaceLayout {
  workspace: Workspace,

  active:        usize,
  tabs:          Vec<Tab>,
  views:         ViewCollection,
  tab_layout:    WidgetCollection,
  current_hover: Option<HitResult>,
  dragging:      Option<(SplitPath, f64)>,
  notify:        Notify,
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

#[derive(Clone, Copy, Debug, PartialEq)]
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
  pub fn new(store: &mut RenderStore, args: Args) -> Self {
    let path = std::env::current_dir().unwrap();
    let mut terminal = crate::view::TerminalView::new(&store.clipboard, &path);
    terminal.on_focus(true);

    let mut state = State {
      keys: vec![],
      terminal,
      terminal_should_exit: false,
      workspaces: HashMap::new(),
      active: AppView::Terminal,
    };

    if let Some(root) = args.editor {
      if !root.exists() {
        error!("path does not exist: {}", root.display());
        std::process::exit(1);
      }
      let root = WorkspaceRoot::from_path(root);
      state.workspaces.insert(root.clone(), WorkspaceLayout::new(root.clone(), store));
      state.terminal.on_focus(false);
      state.active = AppView::Workspace(root);
    }

    state
  }

  fn mode(&self) -> Mode {
    match &self.active {
      AppView::Workspace(w) => self.workspaces[w].mode(),
      AppView::Terminal => Mode::Insert,
    }
  }

  pub fn on_key(&mut self, key: KeyStroke, store: &mut RenderStore) {
    self.keys.push(key);

    if let AppView::Workspace(w) = &mut self.active {
      if let ViewContent::Editor(e) =
        &mut self.workspaces.get_mut(w).unwrap().active_view_mut().content
      {
        e.temporary_mode = None;
      }
    }

    let res = store.config.borrow().settings.keybinds.parse(self.mode(), &self.keys);
    match res {
      Ok(action) => {
        self.perform_action(action, store);
        self.keys.clear();
      }
      Err(be_input::ActionError::Unrecognized) => self.keys.clear(),
      Err(be_input::ActionError::Incomplete) => {
        let has_temporary = self.keys.len() == 1 && !self.keys[0].control;

        if has_temporary {
          let temporary_mode = match self.keys[0].key {
            be_input::Key::Char('r') => Some(be_input::Mode::Replace),
            be_input::Key::Char('c' | 'd') => Some(be_input::Mode::Normal),
            _ => None,
          };

          if let AppView::Workspace(w) = &mut self.active {
            if let ViewContent::Editor(e) =
              &mut self.workspaces.get_mut(w).unwrap().active_view_mut().content
            {
              e.temporary_mode = temporary_mode;
            }
          }
        }
      }
    }
  }

  pub fn layout(&mut self, layout: &mut Layout) {
    match &mut self.active {
      AppView::Workspace(w) => self.workspaces.get_mut(w).unwrap().layout(layout),
      AppView::Terminal => {
        layout.active = Some(ViewId(0));
        self.terminal.layout(layout);
        if !layout.to_close.is_empty() {
          self.terminal_should_exit = true;
        }
      }
    }
  }
  pub fn draw(&mut self, render: &mut Render) {
    match &mut self.active {
      AppView::Workspace(w) => self.workspaces.get_mut(w).unwrap().draw(render),
      AppView::Terminal => self.terminal.draw(render),
    }
  }
  pub fn should_exit(&self) -> bool {
    match &self.active {
      AppView::Workspace(_) => false,
      AppView::Terminal => self.terminal_should_exit,
    }
  }
  pub fn animated(&self) -> bool {
    match &self.active {
      AppView::Workspace(w) => self.workspaces[w].animated(),
      AppView::Terminal => false,
    }
  }
  pub fn on_mouse(&mut self, ev: MouseEvent, size: kurbo::Size, store: &RenderStore) -> CursorKind {
    match &mut self.active {
      AppView::Workspace(w) => self.workspaces.get_mut(w).unwrap().on_mouse(ev, size, store),
      AppView::Terminal => self.terminal.on_mouse(&ev, size, store),
    }
  }
  /// Handles an event. Returns `true` if the app should close.
  pub fn on_event(&mut self, event: Event, store: &mut RenderStore) -> bool {
    match &mut self.active {
      AppView::Workspace(w) => self.workspaces.get_mut(w).unwrap().on_event(event, store),
      AppView::Terminal => false,
    }
  }

  fn perform_action(&mut self, action: Action, store: &mut RenderStore) {
    match action {
      Action::Navigate { nav: Navigation::OpenWorkspace } => {
        if matches!(self.active, AppView::Terminal) {
          let path = WorkspaceRoot::from_path(self.terminal.current_dir());

          if !self.workspaces.contains_key(&path) {
            self.workspaces.insert(path.clone(), WorkspaceLayout::new(path.clone(), store));
          }

          self.terminal.on_focus(false);
          self.active = AppView::Workspace(path);
        } else {
          self.terminal.on_focus(true);
          self.active = AppView::Terminal;
        }
      }

      _ => match &mut self.active {
        AppView::Workspace(w) => self.workspaces.get_mut(w).unwrap().perform_action(action, store),
        AppView::Terminal => self.terminal.perform_action(action),
      },
    }
  }
}

impl WorkspaceLayout {
  pub fn new(root: WorkspaceRoot, store: &mut RenderStore) -> Self {
    let mut workspace = Workspace::new(root, store.config.clone());
    workspace.clipboard = store.clipboard.clone();

    let notify = store.notifier();
    workspace.set_waker(move |ev| notify.workspace_event(ev));

    let mut state = WorkspaceLayout {
      workspace,

      active: 1,
      tabs: vec![],
      views: ViewCollection::new(),
      tab_layout: WidgetCollection::new(),
      current_hover: None,
      dragging: None,
      notify: store.notifier(),
    };

    let layout = store.config.borrow().settings.layout.clone();

    fn build_view(
      state: &mut WorkspaceLayout,
      store: &mut RenderStore,
      tab: be_config::TabSettings,
    ) -> Pane {
      match tab {
        be_config::TabSettings::Split(split) => {
          if split.percent.len() != split.children.len().saturating_sub(1) {
            eprintln!("invalid split percentages"); // TODO: Real errors!
            return Pane::View(state.views.new_view(view::EditorView::new(&mut state.workspace)));
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
        be_config::TabSettings::Terminal => Pane::View(state.views.new_view(
          view::TerminalView::new(&state.workspace.clipboard, state.workspace.root.as_path()),
        )),
        be_config::TabSettings::Editor => {
          Pane::View(state.views.new_view(view::EditorView::new(&mut state.workspace)))
        }
        be_config::TabSettings::FileTree => Pane::View(
          state.views.new_view(view::FileTree::new(store.notifier(), &mut state.workspace)),
        ),
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
    puffin::profile_function!();

    self.workspace.fs.update();

    layout.clipped(
      Rect::new(0.0, layout.size().height - 25.0, layout.size().width, layout.size().height),
      |layout| {
        self.layout_tabs(layout);
      },
    );

    for editor in self.workspace.editors_mut() {
      puffin::profile_scope!("layout editor");
      editor.layout();
    }

    layout.clipped(
      Rect::new(0.0, 0.0, layout.size().width, layout.size().height - 25.0),
      |layout| {
        let mut tab = &mut self.tabs[self.active];
        if let Some(popup) = &mut tab.popup {
          popup.layout(layout);
        }
        {
          puffin::profile_scope!("layout views");
          tab.content.layout(&mut self.views.views, layout);
        }
        if !layout.to_close.is_empty() {
          for to_close in layout.to_close.drain(..) {
            if matches!(tab.content, Pane::View(v) if v == to_close) {
              self.tabs.remove(self.active);
              self.active -= 1;
              tab = &mut self.tabs[self.active];
            } else {
              tab.content.close(to_close, &mut self.views.views);
            }

            // FIXME: Deduplicate with ':q'
            self.views.views.remove(&to_close);
          }

          // Re-run layout after removing closed views.
          {
            puffin::profile_scope!("layout views after close");
            tab.content.layout(&mut self.views.views, layout);
          }
        }
      },
    );

    for editor in self.workspace.editors_mut() {
      editor.clear_damage();
    }
  }

  fn hit_pane(&self, pos: Point, size: kurbo::Size) -> Option<HitResult> {
    if pos.y < size.height - 25.0 {
      let tab = &self.tabs[self.active];
      tab.content.hit_pane(pos, Size::new(size.width, size.height - 25.0))
    } else {
      Some(HitResult::View(ViewId::TABS))
    }
  }

  fn on_mouse(&mut self, ev: MouseEvent, size: kurbo::Size, store: &RenderStore) -> CursorKind {
    match &ev {
      MouseEvent::Move { pos } => {
        let pos = *pos;

        if let Some((ref path, ref mut last)) = self.dragging {
          let coord = if path.axis == Axis::Vertical { pos.x } else { pos.y };
          let delta = coord - *last;
          *last = coord;
          let content_size = Size::new(size.width, size.height - 25.0);
          let path = path.clone();
          self.tabs[self.active].content.resize(&path, delta, content_size);
        }

        let new_hover = self.hit_pane(pos, size);

        let old_id = self.current_hover.as_ref().and_then(HitResult::view_id);
        let new_id = new_hover.as_ref().and_then(HitResult::view_id);
        match (old_id, new_id) {
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

        self.current_hover = new_hover;
      }

      MouseEvent::Leave => {
        if let Some(id) = self.current_hover.as_ref().and_then(HitResult::view_id) {
          self.send_mouse_event(id, &MouseEvent::Leave, size, store);
        }
        self.current_hover = None;
        self.dragging = None;
      }

      MouseEvent::Button { pos, pressed: true, .. } => match &self.current_hover {
        Some(HitResult::Divider(path)) => {
          let coord = if path.axis == Axis::Vertical { pos.x } else { pos.y };
          self.dragging = Some((path.clone(), coord));
        }
        Some(HitResult::View(id)) if *id != ViewId::TABS => {
          let id = *id;
          let tab = &mut self.tabs[self.active];
          let prev_focus = tab.content.active();
          if id != prev_focus {
            tab.content.focus_view(id);
            self.views.get_mut(prev_focus).unwrap().on_focus(false);
            self.views.get_mut(id).unwrap().on_focus(true);
          }
        }
        _ => {}
      },

      MouseEvent::Button { pressed: false, .. } => {
        self.dragging = None;
      }

      _ => {}
    }

    if let Some((ref path, _)) = self.dragging {
      return match path.axis {
        Axis::Vertical => CursorKind::ResizeEastWest,
        Axis::Horizontal => CursorKind::ResizeNorthSouth,
      };
    }

    if let Some(HitResult::Divider(ref path)) = self.current_hover {
      return match path.axis {
        Axis::Vertical => CursorKind::ResizeEastWest,
        Axis::Horizontal => CursorKind::ResizeNorthSouth,
      };
    }

    if let Some(id) = self.current_hover.as_ref().and_then(HitResult::view_id) {
      self.send_mouse_event(id, &ev, size, store).unwrap_or(CursorKind::Default)
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
        tab.content.draw_dividers(render, Point::ZERO, render.size());

        if let Some(popup) = &mut tab.popup {
          render.clipped(popup.bounds(render.size()), |render| popup.draw(render));
        }
      },
      |state, render| state.draw_tabs(render),
    );
  }

  fn open(&mut self, path: &std::path::Path, cursor: Option<CursorOrLsp>) {
    let active_id = self.tabs[self.active].content.active();
    let active_is_editor =
      matches!(&self.views.get(active_id).unwrap().content, ViewContent::Editor(_));

    // When a non-editor view (e.g. file tree) opens a file, target its
    // geometric neighbor.
    let target_id = if active_is_editor {
      active_id
    } else {
      self.tabs[self.active].content.neighbor_of(active_id).unwrap_or(active_id)
    };

    // If the neighbor isn't an editor either, fall back to any visible editor.
    let target_id = if matches!(&self.views.get(target_id).unwrap().content, ViewContent::Editor(_))
    {
      target_id
    } else if let Some(id) = self
      .views
      .views
      .iter()
      .filter(|(_, v)| v.visible())
      .find_map(|(id, v)| matches!(v.content, ViewContent::Editor(_)).then_some(*id))
    {
      id
    } else {
      return;
    };

    if !active_is_editor && target_id != active_id {
      self.tabs[self.active].content.focus_view(target_id);
      self.views.get_mut(active_id).unwrap().on_focus(false);
      self.views.get_mut(target_id).unwrap().on_focus(true);
    }

    if let ViewContent::Editor(e) = &mut self.views.get_mut(target_id).unwrap().content {
      let res = e.open(path, &mut self.workspace);
      if let Some(cursor) = cursor
        && res.is_ok()
      {
        let cursor = cursor.into_cursor(e.editor.doc());
        e.editor.move_to(cursor);
      }
    }

    if let Some(tree) = self.current_file_tree_mut() {
      tree.open(path);
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

  fn active_tab(&self) -> &Tab { &self.tabs[self.active] }
  fn active_tab_mut(&mut self) -> &mut Tab { &mut self.tabs[self.active] }

  fn perform_action(&mut self, action: Action, store: &mut RenderStore) {
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
        self.active_tab_mut().popup = Some(view::Popup::Search(view::Search::new(
          self.workspace.root.clone(),
          self.notify.clone(),
        )));
      }
      Action::Navigate { nav: Navigation::Split(axis) } => {
        let new_view = self.split_active_view(store);
        self.tabs[self.active].content.split(
          match axis {
            be_input::Axis::Horizontal => kurbo::Axis::Horizontal,
            be_input::Axis::Vertical => kurbo::Axis::Vertical,
          },
          &mut self.views.views,
          store,
          new_view,
        );
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
    let tab_height = layout.size().height;
    let mut tab_layout = self.tab_layout.begin(layout);

    let mut left = vec![];

    for (i, tab) in self.tabs.iter().enumerate() {
      let button = tab_layout.add_widget(crate::widget::Button::new(&tab.title));

      if button.pressed() {
        self.active = i;
      }

      left.push(button.id);
    }

    let mut right = vec![];
    right.push(tab_layout.add_widget(crate::widget::Text::new("Rust")).id);

    let left = tab_layout
      .add_widget(
        crate::widget::Stack::new(Axis::Horizontal, Align::Start, Justify::Center, left)
          .gap(5.0)
          .cross_size(tab_height),
      )
      .id;
    let right = tab_layout
      .add_widget(
        crate::widget::Stack::new(Axis::Horizontal, Align::Start, Justify::Center, right)
          .gap(5.0)
          .cross_size(tab_height),
      )
      .id;
    let root = tab_layout
      .add_widget(crate::widget::Stack::new(
        Axis::Horizontal,
        Align::SpaceBetween,
        Justify::Center,
        vec![left, right],
      ))
      .id;
    tab_layout.finish(root);
  }

  fn draw_tabs(&mut self, render: &mut Render) {
    render
      .fill(&Rect::from_origin_size(Point::ZERO, render.size()), render.theme().background_lower);

    self.tab_layout.draw(render);
  }

  fn active_editor(&mut self) -> Option<&mut EditorView> {
    if let ViewContent::Editor(e) = &mut self.active_view_mut().content { Some(e) } else { None }
  }

  /// Handles an event. Returns `true` if the app should close.
  fn on_event(&mut self, event: Event, store: &mut RenderStore) -> bool {
    match event {
      Event::Workspace(WorkspaceEvent::Refresh) => {}
      Event::Workspace(WorkspaceEvent::Editor(be_editor::EditorEvent::OpenFile(path, cursor))) => {
        self.tabs[self.active].popup = None;
        self.open(&path, cursor);
      }
      Event::Workspace(WorkspaceEvent::Editor(be_editor::EditorEvent::RecordDefinition(
        path,
        cursor,
      ))) => {
        if let Some(editor) = self.active_editor() {
          editor.record_definition(path, cursor);
        }
      }
      Event::Workspace(WorkspaceEvent::Editor(be_editor::EditorEvent::RunCommand(cmd))) => {
        let (cmd, args) = cmd.split_once(' ').unwrap_or((&cmd, ""));

        match cmd {
          "w" => {
            if let Some(editor) = self.active_editor() {
              editor.editor.begin_save();
            }
          }
          "q" => {
            let tab = &mut self.tabs[self.active];
            let active_view = tab.content.active();
            tab.content.close(active_view, &mut self.views.views);
            // FIXME: Deduplicate layouts closing themselves (ie, terminal exit).
            self.views.views.remove(&active_view);
          }
          "e" => {
            if args.is_empty() {
              if let Some(editor) = self.active_editor() {
                if let Some(path) = editor.editor.file().map(|p| p.to_path_buf()) {
                  let _ = editor.editor.open(&path);
                }
              }
            } else {
              let path = std::path::Path::new(args);
              let path = if path.is_absolute() {
                path.to_path_buf()
              } else {
                self.workspace.root.as_path().join(path)
              };
              self.open(&path, None);
            }
          }
          "noh" => {
            if let Some(editor) = self.active_editor() {
              editor.editor.clear_search();
            }
          }
          "vs" => {
            let new_view = self.split_active_view(store);
            self.tabs[self.active].content.split(
              Axis::Vertical,
              &mut self.views.views,
              store,
              new_view,
            );
          }
          "hs" => {
            let new_view = self.split_active_view(store);
            self.tabs[self.active].content.split(
              Axis::Horizontal,
              &mut self.views.views,
              store,
              new_view,
            );
          }

          _ => {
            println!("unknown command: {}", cmd);
          }
        }

        self.tabs[self.active].popup = None;
      }
      Event::Workspace(WorkspaceEvent::Editor(be_editor::EditorEvent::SavedFile(path))) => {
        self.workspace.fs.notify_saved(&path);
      }
      Event::Exit => return true,
    }

    false
  }

  fn split_active_view(&mut self, store: &mut RenderStore) -> ViewId {
    match self.views.get(self.tabs[self.active].content.active()).unwrap().content {
      ViewContent::Editor(ref e) => {
        let mut editor = crate::view::EditorView::new(&mut self.workspace);
        editor.split_from(e);
        self.views.new_view(editor)
      }
      ViewContent::FileTree(_) => {
        self.views.new_view(crate::view::FileTree::new(store.notifier(), &mut self.workspace))
      }
      ViewContent::Terminal(_) => self.views.new_view(crate::view::TerminalView::new(
        &self.workspace.clipboard,
        self.workspace.root.as_path(),
      )),
    }
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

impl Index<ViewId> for ViewCollection {
  type Output = View;

  fn index(&self, id: ViewId) -> &View { self.views.get(&id).unwrap() }
}

impl IndexMut<ViewId> for ViewCollection {
  fn index_mut(&mut self, id: ViewId) -> &mut View { self.views.get_mut(&id).unwrap() }
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
