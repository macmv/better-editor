mod render;

use std::{
  cell::Cell,
  collections::HashMap,
  fmt,
  hash::Hash,
  ops::{Index, IndexMut},
  path::{Path, PathBuf},
};

use be_editor::CursorOrLsp;
use be_fs::{WorkspacePath, WorkspaceRoot};
use be_input::{Action, KeyStroke, Mode, Navigation};
use be_shared::SharedHandle;
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
mod remote;
mod spinner;
mod theme;
mod view;
mod widget;
mod workspace_list;

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

  /// Connect to a `be-host` over ssh: `[user@]host` or
  /// `ssh://[user@]host[/path]`.
  #[clap(long)]
  remote: Option<String>,

  /// The command that runs `be-host` on the remote machine.
  #[clap(long)]
  remote_host_cmd: Option<String>,
}

struct State {
  keys: Vec<KeyStroke>,

  terminal:             crate::view::TerminalView,
  /// The terminal's shell runs on a remote host, so its directory is not a
  /// local path. This names it in calls to the host.
  terminal_remote_id:   Option<u64>,
  terminal_should_exit: bool,

  workspaces: HashMap<WorkspaceRoot, WorkspaceLayout>,

  active: AppView,

  /// The workspace `AppView::Loading` is waiting on.
  loading: Option<Loading>,

  /// The connection the terminal's shell runs on, with `--remote`. Remote
  /// workspaces open their own connections.
  remote:       Option<remote::RemoteSession>,
  /// The id of the next remote session. The terminal's is 0.
  next_session: u64,

  /// The row selected on the workspace list.
  list_selected: usize,
  /// What the workspace list returns to when it is left without a choice.
  list_return:   AppView,
}

#[derive(Clone)]
enum AppView {
  Terminal,
  Loading,
  /// The list of every open workspace.
  Workspaces,
  Workspace(WorkspaceRoot),
}

/// A remote workspace that is still being opened.
struct Loading {
  /// What was asked for, before the host made it canonical.
  path:    PathBuf,
  /// `None` once it has finished badly.
  opening: Option<remote::OpeningWorkspace>,
  /// The workspace's own connection. Moves into the workspace once it opens.
  session: Option<remote::RemoteSession>,
  error:   Option<String>,
  spinner: spinner::Spinner,
  /// Frames drawn so far. A load that finishes at once never shows the
  /// spinner.
  frames:  Cell<u32>,
}

/// Frames the loading screen stays blank before the spinner appears.
const BLANK_FRAMES: u32 = 3;

struct WorkspaceLayout {
  workspace: Workspace,
  /// Set if the workspace's files are on another machine. The workspace owns
  /// the connection, so closing it drops the connection.
  remote:    Option<remote::RemoteSession>,

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

/// Height of the tab bar, in pixels. This is the tab button height (a 13px UI
/// font line plus its border) plus 5px of margin above and below.
const TAB_BAR_HEIGHT: f64 = 30.0;

struct Tab {
  title:   String,
  content: Pane,
  popup:   Option<view::Popup>,
  /// The previously focused view, for `LastPane`.
  last:    Option<ViewId>,
  /// If set, this view fills the whole tab.
  zoomed:  Option<ViewId>,
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

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct Location {
  pub file: &'static str,
  pub line: u32,
  pub col:  u32,
}

impl Location {
  pub fn new(loc: &std::panic::Location<'static>) -> Self {
    Location { file: loc.file(), line: loc.line(), col: loc.column() }
  }
}

impl fmt::Debug for Location {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}:{}:{}", self.file, self.line, self.col)
  }
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct WidgetPath {
  loc: Location,
  idx: u32,
}

impl fmt::Debug for WidgetPath {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{:?}[{}]", self.loc, self.idx)
  }
}

impl ViewId {
  pub const TABS: ViewId = ViewId(u64::MAX);
}

impl State {
  pub fn new(store: &mut RenderStore, args: Args) -> Self {
    // With `--remote`, the terminal we start in is a shell on the host.
    let mut remote = None;
    if let Some(target) = &args.remote {
      let host_cmd = args.remote_host_cmd.as_deref().unwrap_or("be-host");
      let notify = store.notifier();
      match remote::RemoteSession::connect(
        0,
        remote::RemoteTarget::parse(target),
        host_cmd,
        move |wake| notify.remote_wake(0, wake),
      ) {
        Ok(session) => remote = Some(session),
        Err(e) => error!("failed to connect to {target}: {e}"),
      }
    }

    let size = be_terminal::Size { rows: 40, cols: 80 };
    let mut terminal_remote_id = None;
    let mut terminal = match &mut remote {
      Some(session) => {
        let cwd = session.path().unwrap_or_default().to_string();
        let pty = session.new_pty(size, cwd);
        terminal_remote_id = Some(pty.id());
        let terminal = be_terminal::Terminal::with_host(size, Box::new(pty));
        crate::view::TerminalView::from_handle(&store.clipboard, SharedHandle::new(terminal))
      }
      None => {
        let path = std::env::current_dir().unwrap();
        crate::view::TerminalView::new(&store.clipboard, &path)
      }
    };
    terminal.on_focus(true);

    let mut state = State {
      keys: vec![],
      terminal,
      terminal_remote_id,
      terminal_should_exit: false,
      workspaces: HashMap::new(),
      active: AppView::Terminal,
      loading: None,
      remote,
      next_session: 1,
      list_selected: 0,
      list_return: AppView::Terminal,
    };

    if let Some(root) = args.editor {
      let opened = match state.remote.as_ref().map(|r| r.handle()) {
        // With `--remote`, the path is on the host. A relative one is
        // relative to the directory given with the host, if any.
        Some(handle) => {
          let path = match state.remote.as_ref().and_then(|r| r.path()) {
            Some(base) if root.is_relative() => Path::new(base).join(&root),
            _ => root.clone(),
          };
          state.start_remote_workspace(store, &handle, None, path);
          None
        }
        None => {
          if !root.exists() {
            error!("path does not exist: {}", root.display());
            std::process::exit(1);
          }
          let root = WorkspaceRoot::from_path(root);
          state.workspaces.insert(root.clone(), WorkspaceLayout::new(root.clone(), store, None));
          Some(root)
        }
      };
      if let Some(root) = opened {
        state.terminal.on_focus(false);
        state.active = AppView::Workspace(root);
      }
    }

    state
  }

  /// Starts opening the workspace at `path` on the host `from` is connected
  /// to, over a new connection, and shows a loading screen until the host
  /// answers. With `terminal`, the workspace is the directory that terminal
  /// of `from`'s is in, instead of `path`.
  fn start_remote_workspace(
    &mut self,
    store: &RenderStore,
    from: &remote::RemoteHandle,
    terminal: Option<u64>,
    path: PathBuf,
  ) {
    let id = self.next_session;
    self.next_session += 1;

    let notify = store.notifier();
    let (session, opening, error) =
      match remote::RemoteSession::connect_like(id, from, move |wake| notify.remote_wake(id, wake))
      {
        Ok(session) => {
          let handle = session.handle();
          let opening = match terminal {
            Some(term) => from.open_terminal_workspace(term, &handle),
            None => handle.open_workspace(&path),
          };
          (Some(session), Some(opening), None)
        }
        Err(e) => {
          error!("failed to connect to {}: {e}", from.target());
          (None, None, Some(e.to_string()))
        }
      };

    self.loading = Some(Loading {
      path,
      opening,
      session,
      error,
      spinner: spinner::Spinner::new(),
      frames: Cell::new(0),
    });
    self.terminal.on_focus(false);
    self.active = AppView::Loading;
  }

  /// Finishes the workspace being loaded, if the host has answered.
  fn poll_loading(&mut self, store: &mut RenderStore) {
    let Some(loading) = &mut self.loading else { return };
    let Some(opening) = &mut loading.opening else { return };
    let Some(res) = opening.poll() else { return };

    match res {
      Ok((path, backend)) => {
        let session = loading.session.take().expect("a session while opening");
        let root = WorkspaceRoot::remote(session.handle().target().to_string(), path);
        // If the workspace is already open, it keeps its own connection, and
        // this one closes.
        if !self.workspaces.contains_key(&root) {
          self.workspaces.insert(
            root.clone(),
            WorkspaceLayout::new(root.clone(), store, Some((session, backend))),
          );
        }
        self.loading = None;
        // Someone who has moved on to the workspace list keeps it, and finds
        // this workspace there.
        if matches!(self.active, AppView::Loading) {
          self.active = AppView::Workspace(root);
        }
      }
      Err(e) => {
        error!("cannot open {} on the remote host: {e}", loading.path.display());
        loading.error = Some(e.to_string());
        loading.opening = None;
        loading.session = None;
      }
    }
  }

  /// The remote session named `id`, wherever it lives.
  fn session_mut(&mut self, id: u64) -> Option<&mut remote::RemoteSession> {
    let loading = self.loading.as_mut().and_then(|l| l.session.as_mut());
    let workspaces = self.workspaces.values_mut().filter_map(|w| w.remote.as_mut());
    self.remote.as_mut().into_iter().chain(loading).chain(workspaces).find(|s| s.id() == id)
  }

  fn mode(&self) -> Mode {
    match &self.active {
      AppView::Workspace(w) => self.workspaces[w].mode(),
      AppView::Terminal | AppView::Loading | AppView::Workspaces => Mode::Insert,
    }
  }

  /// Every open workspace, in the order the workspace list shows them.
  fn workspace_roots(&self) -> Vec<WorkspaceRoot> {
    let mut roots: Vec<_> = self.workspaces.keys().cloned().collect();
    workspace_list::sort(&mut roots);
    roots
  }

  /// Shows `view`, or the terminal if what it names has gone away.
  fn show(&mut self, view: AppView) {
    let view = match &view {
      AppView::Workspace(root) if !self.workspaces.contains_key(root) => AppView::Terminal,
      AppView::Loading if self.loading.is_none() => AppView::Terminal,
      _ => view,
    };
    self.terminal.on_focus(matches!(view, AppView::Terminal));
    self.active = view;
  }

  fn open_workspace_list(&mut self) {
    let roots = self.workspace_roots();
    self.list_selected = match &self.active {
      AppView::Workspace(active) => roots.iter().position(|r| r == active).unwrap_or(0),
      _ => 0,
    };
    self.list_return = self.active.clone();
    self.terminal.on_focus(false);
    self.active = AppView::Workspaces;
  }

  /// Handles a key typed on the workspace list. Returns `false` if the key
  /// is not one of the list's, such as the start of a navigation bind.
  fn on_workspace_list_key(&mut self, key: &KeyStroke) -> bool {
    if key.control || key.alt || key.sup {
      return false;
    }

    let count = self.workspaces.len();
    match key.key {
      be_input::Key::Char('j') | be_input::Key::ArrowDown => {
        self.list_selected = (self.list_selected + 1).min(count.saturating_sub(1));
      }
      be_input::Key::Char('k') | be_input::Key::ArrowUp => {
        self.list_selected = self.list_selected.saturating_sub(1);
      }
      be_input::Key::Char('\n') => {
        let selected = self.workspace_roots().into_iter().nth(self.list_selected);
        match selected {
          Some(root) => self.show(AppView::Workspace(root)),
          None => self.show(self.list_return.clone()),
        }
      }
      be_input::Key::Escape => self.show(self.list_return.clone()),
      _ => return false,
    }
    true
  }

  pub fn on_key(&mut self, key: KeyStroke, store: &mut RenderStore) {
    if matches!(self.active, AppView::Workspaces) && self.keys.is_empty() {
      if self.on_workspace_list_key(&key) {
        return;
      }
    }

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
      AppView::Loading | AppView::Workspaces => {}
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
      AppView::Loading => self.draw_loading(render),
      AppView::Workspaces => {
        workspace_list::draw(render, &self.workspace_roots(), self.list_selected)
      }
    }
  }
  fn draw_loading(&self, render: &mut Render) {
    let Some(loading) = &self.loading else { return };

    let size = render.size();
    render.fill(&Rect::from_origin_size(Point::ZERO, size), render.theme().background);

    let frames = loading.frames.get();
    loading.frames.set(frames.saturating_add(1));
    if frames < BLANK_FRAMES && loading.error.is_none() {
      return;
    }

    let center = Point::new(size.width / 2.0, size.height / 2.0);
    let message = match &loading.error {
      Some(e) => format!("Could not open {}: {e}", loading.path.display()),
      None => {
        loading.spinner.draw(render, center);
        format!("Opening {}", loading.path.display())
      }
    };

    let layout = render.layout_text(Font::Ui, &message, render.theme().text);
    let pos = Point::new(
      center.x - layout.size().width / 2.0,
      if loading.error.is_some() { center.y } else { center.y + 70.0 },
    );
    render.draw_text(&layout, pos);
  }
  pub fn should_exit(&self) -> bool {
    match &self.active {
      AppView::Workspace(_) => false,
      AppView::Terminal => self.terminal_should_exit,
      AppView::Loading | AppView::Workspaces => false,
    }
  }
  pub fn animated(&self) -> bool {
    match &self.active {
      AppView::Workspace(w) => self.workspaces[w].animated(),
      AppView::Loading => self.loading.as_ref().is_some_and(|l| l.opening.is_some()),
      AppView::Terminal | AppView::Workspaces => false,
    }
  }
  pub fn on_mouse(&mut self, ev: MouseEvent, size: kurbo::Size, store: &RenderStore) -> CursorKind {
    match &mut self.active {
      AppView::Workspace(w) => self.workspaces.get_mut(w).unwrap().on_mouse(ev, size, store),
      AppView::Terminal => self.terminal.on_mouse(&ev, size, store),
      AppView::Loading | AppView::Workspaces => CursorKind::Default,
    }
  }
  /// Handles an event. Returns `true` if the app should close.
  pub fn on_event(&mut self, event: Event, store: &mut RenderStore) -> bool {
    if let Event::Remote(id, wake) = event {
      if let Some(session) = self.session_mut(id) {
        session.on_wake(wake);
      }
      self.poll_loading(store);
      return false;
    }

    match &mut self.active {
      AppView::Workspace(w) => self.workspaces.get_mut(w).unwrap().on_event(event, store),
      AppView::Terminal | AppView::Loading | AppView::Workspaces => false,
    }
  }

  fn perform_action(&mut self, action: Action, store: &mut RenderStore) {
    match action {
      Action::Navigate { nav: Navigation::Workspaces } => {
        if matches!(self.active, AppView::Workspaces) {
          self.show(self.list_return.clone());
        } else {
          self.open_workspace_list();
        }
      }

      Action::Navigate { nav: Navigation::OpenWorkspace } => {
        if matches!(self.active, AppView::Terminal) {
          let path = if let Some(id) = self.terminal_remote_id {
            let Some(handle) = self.remote.as_ref().map(|r| r.handle()) else { return };
            self.start_remote_workspace(store, &handle, Some(id), PathBuf::from("."));
            return;
          } else {
            let path = WorkspaceRoot::from_path(self.terminal.current_dir());
            if !self.workspaces.contains_key(&path) {
              self.workspaces.insert(path.clone(), WorkspaceLayout::new(path.clone(), store, None));
            }
            path
          };

          self.terminal.on_focus(false);
          self.active = AppView::Workspace(path);
        } else {
          // Leaving a loading screen gives up on the workspace.
          self.loading = None;
          self.terminal.on_focus(true);
          self.active = AppView::Terminal;
        }
      }

      _ => match &mut self.active {
        AppView::Workspace(w) => self.workspaces.get_mut(w).unwrap().perform_action(action, store),
        AppView::Terminal => self.terminal.perform_action(action),
        AppView::Loading | AppView::Workspaces => {}
      },
    }
  }
}

impl Tab {
  /// The zoomed view, if it is still part of this tab.
  fn zoomed(&self) -> Option<ViewId> {
    self.zoomed.filter(|z| self.content.views().any(|id| id == *z))
  }

  fn layout(&self, views: &mut HashMap<ViewId, View>, layout: &mut Layout) {
    match self.zoomed() {
      Some(z) => Pane::View(z).layout(views, layout),
      None => self.content.layout(views, layout),
    }
  }
}

impl WorkspaceLayout {
  pub fn new(
    root: WorkspaceRoot,
    store: &mut RenderStore,
    remote: Option<(remote::RemoteSession, be_workspace::Backend)>,
  ) -> Self {
    let (remote, mut workspace) = match remote {
      Some((session, backend)) => {
        (Some(session), Workspace::with_backend(root, store.config.clone(), backend))
      }
      None => (None, Workspace::new(root, store.config.clone())),
    };
    workspace.clipboard = store.clipboard.clone();

    let notify = store.notifier();
    workspace.set_waker(move |ev| notify.workspace_event(ev));

    let mut state = WorkspaceLayout {
      workspace,
      remote,

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
        be_config::TabSettings::Terminal => {
          let clipboard = &state.workspace.clipboard;
          let root = state.workspace.root.as_path();
          let terminal = match &mut state.remote {
            Some(remote) => {
              let size = be_terminal::Size { rows: 40, cols: 80 };
              let pty = remote.new_pty(size, root.to_string_lossy().into_owned());
              let terminal = be_terminal::Terminal::with_host(size, Box::new(pty));
              view::TerminalView::from_handle(clipboard, SharedHandle::new(terminal))
            }
            None => view::TerminalView::new(clipboard, root),
          };
          Pane::View(state.views.new_view(terminal))
        }
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
      state.tabs.push(Tab {
        title:   title.unwrap_or_default(),
        content: view,
        popup:   None,
        last:    None,
        zoomed:  None,
      });
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
      Rect::new(
        0.0,
        layout.size().height - TAB_BAR_HEIGHT,
        layout.size().width,
        layout.size().height,
      ),
      |layout| {
        self.layout_tabs(layout);
      },
    );

    for editor in self.workspace.editors_mut() {
      puffin::profile_scope!("layout editor");
      editor.layout();
    }

    layout.clipped(
      Rect::new(0.0, 0.0, layout.size().width, layout.size().height - TAB_BAR_HEIGHT),
      |layout| {
        let mut tab_count = self.tabs.len();
        let mut tab = &mut self.tabs[self.active];
        if let Some(popup) = &mut tab.popup {
          popup.layout(layout);
        }
        {
          puffin::profile_scope!("layout views");
          tab.layout(&mut self.views.views, layout);
        }
        if !layout.to_close.is_empty() {
          for to_close in layout.to_close.drain(..) {
            if matches!(tab.content, Pane::View(v) if v == to_close) {
              // The only view in the tab: the tab goes with it, unless it is
              // the last tab, which has to stay (and keep its view).
              if tab_count == 1 {
                continue;
              }
              self.tabs.remove(self.active);
              tab_count -= 1;
              self.active = self.active.saturating_sub(1);
              tab = &mut self.tabs[self.active];
            } else {
              tab.content.close(to_close, &mut self.views.views);
              // The close is a no-op if it was the last child of a split, and
              // that view must stay for the pane to point at.
              if tab.content.views().any(|v| v == to_close) {
                continue;
              }
            }

            // FIXME: Deduplicate with ':q'
            self.views.views.remove(&to_close);
          }

          // Re-run layout after removing closed views.
          {
            puffin::profile_scope!("layout views after close");
            tab.layout(&mut self.views.views, layout);
          }
        }
      },
    );

    for editor in self.workspace.editors_mut() {
      editor.clear_damage();
    }
  }

  fn hit_pane(&self, pos: Point, size: kurbo::Size) -> Option<HitResult> {
    if pos.y < size.height - TAB_BAR_HEIGHT {
      let tab = &self.tabs[self.active];
      match tab.zoomed() {
        Some(z) => Some(HitResult::View(z)),
        None => tab.content.hit_pane(pos, Size::new(size.width, size.height - TAB_BAR_HEIGHT)),
      }
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
          let content_size = Size::new(size.width, size.height - TAB_BAR_HEIGHT);
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

        // Focus follows the mouse, but only when the mouse actually moves. This
        // lets keyboard navigation move focus away from the hovered pane, and
        // have it stay there until the mouse is moved again.
        if self.dragging.is_none() {
          if let Some(id) = new_id.filter(|id| *id != ViewId::TABS) {
            self.focus_view(id);
          }
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
          self.focus_view(id);
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
        if let Some(ev) =
          ev.within(&Rect::new(0.0, size.height - TAB_BAR_HEIGHT, size.width, size.height))
        {
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
      Distance::Pixels(-TAB_BAR_HEIGHT),
      |state, render| {
        let tab = &mut state.tabs[state.active];
        match tab.zoomed() {
          Some(z) => {
            let view = state.views.get_mut(z).unwrap();
            render.clipped(view.bounds, |render| view.draw(render));
          }
          None => {
            for view in tab.content.views() {
              let view = state.views.get_mut(view).unwrap();
              render.clipped(view.bounds, |render| view.draw(render));
            }
            tab.content.draw_dividers(render, Point::ZERO, render.size());
          }
        }

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
        // A file on another machine is still on its way.
        e.editor.move_to_when_loaded(cursor);
      }
    }

    if let Ok(rel) = path.strip_prefix(self.workspace.root.as_path())
      && let Some(rel_str) = rel.to_str()
    {
      let rel_str = rel_str.to_owned();
      if let Some(tree) = self.current_file_tree_mut() {
        tree.open(WorkspacePath::new(&rel_str));
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

  /// Focuses `new` within the active tab, remembering the old focus for
  /// `LastPane`.
  fn focus_view(&mut self, new: ViewId) {
    let tab = &mut self.tabs[self.active];
    let prev = tab.content.active();
    if prev == new {
      return;
    }
    tab.zoomed = None;
    tab.last = Some(prev);
    tab.content.focus_view(new);
    self.views.get_mut(prev).unwrap().on_focus(false);
    self.views.get_mut(new).unwrap().on_focus(true);
  }

  fn active_tab(&self) -> &Tab { &self.tabs[self.active] }
  fn active_tab_mut(&mut self) -> &mut Tab { &mut self.tabs[self.active] }

  fn perform_action(&mut self, action: Action, store: &mut RenderStore) {
    match action {
      Action::Navigate { nav: Navigation::Tab(i) } => {
        let new_index = i as usize;
        if new_index >= self.tabs.len() || new_index == self.active {
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
      Action::Navigate { nav: Navigation::NewTerminal } => {
        let view = self.views.new_view(view::TerminalView::new(
          &self.workspace.clipboard,
          self.workspace.root.as_path(),
        ));
        self.tabs.push(Tab {
          title:   "terminal".into(),
          content: Pane::View(view),
          popup:   None,
          last:    None,
          zoomed:  None,
        });
        let index = self.tabs.len() - 1;
        self.perform_action(Action::Navigate { nav: Navigation::Tab(index as u8) }, store);
      }
      Action::Navigate { nav: Navigation::Direction(dir) } => {
        self.active_tab_mut().zoomed = None;
        let prev_focus = self.active_tab().content.active();
        if let Some(new_focus) = self.active_tab_mut().content.focus(dir) {
          self.active_tab_mut().last = Some(prev_focus);
          self.views.get_mut(prev_focus).unwrap().on_focus(false);
          self.views.get_mut(new_focus).unwrap().on_focus(true);
        }
      }
      Action::Navigate { nav: Navigation::Cycle(dir) } => {
        let ids = self.active_tab().content.views().collect::<Vec<_>>();
        let active = self.active_tab().content.active();
        let Some(idx) = ids.iter().position(|id| *id == active) else { return };
        let new = match dir {
          be_input::ChangeDirection::Next => ids[(idx + 1) % ids.len()],
          be_input::ChangeDirection::Prev => ids[(idx + ids.len() - 1) % ids.len()],
        };
        self.focus_view(new);
      }
      Action::Navigate { nav: Navigation::LastPane } => {
        let tab = self.active_tab();
        if let Some(last) = tab.last
          && tab.content.views().any(|id| id == last)
        {
          self.focus_view(last);
        }
      }
      Action::Navigate { nav: Navigation::ClosePane } => {
        let tab = &mut self.tabs[self.active];
        let active_view = tab.content.active();
        let before = tab.content.views().count();
        tab.zoomed = None;
        if before > 1 {
          tab.content.close(active_view, &mut self.views.views);
          // The close is a no-op if it was the last child of a split.
          if tab.content.views().count() < before {
            self.views.views.remove(&active_view);
          }
        }
      }
      Action::Navigate { nav: Navigation::Zoom } => {
        let tab = self.active_tab_mut();
        tab.zoomed = if tab.zoomed.is_some() { None } else { Some(tab.content.active()) };
      }
      Action::Navigate { nav: Navigation::OpenSearch } => {
        // The files of a remote workspace come from the host, so the popup
        // opens empty and fills in when they arrive.
        let listed =
          self.remote.as_ref().map(|r| r.handle().begin_list_files(self.workspace.root.as_path()));
        self.active_tab_mut().popup = Some(view::Popup::Search(view::Search::new(
          self.workspace.root.clone(),
          self.notify.clone(),
          listed,
        )));
      }
      Action::Navigate { nav: Navigation::OpenSymbolSearch } => {
        self.active_tab_mut().popup = Some(view::Popup::SymbolSearch(view::SymbolSearch::new(
          self.workspace.root.clone(),
          self.workspace.lsp.clone(),
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

    // An empty leading child: the stack's gap then provides the left margin.
    let mut left = vec![
      tab_layout
        .add_widget(crate::widget::Stack::new(
          Axis::Horizontal,
          Align::Start,
          Justify::Start,
          vec![],
        ))
        .id,
    ];

    for (i, tab) in self.tabs.iter().enumerate() {
      let button =
        tab_layout.add_widget(crate::widget::Button::new(&tab.title).active(i == self.active));

      if button.pressed() {
        self.active = i;
      }

      left.push(button.id);
    }

    let mut right = vec![];
    match self.views.get(self.tabs[self.active].content.active()).unwrap().content {
      ViewContent::Editor(ref e) => {
        if let Some(ft) = e.editor.file_type() {
          let name = &e.editor.config.borrow().languages[&ft].display_name;
          right.push(tab_layout.add_widget(crate::widget::Text::new(name)).id);
        }
      }
      _ => {}
    }

    let left = tab_layout
      .add_widget(
        crate::widget::Stack::new(Axis::Horizontal, Align::Start, Justify::Center, left).gap(5.0),
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
            let before = tab.content.views().count();
            if before > 1 {
              tab.content.close(active_view, &mut self.views.views);
              // FIXME: Deduplicate layouts closing themselves (ie, terminal
              // exit).
              if tab.content.views().count() < before {
                self.views.views.remove(&active_view);
              }
            }
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
      // Handled by `State`.
      Event::Remote(..) => {}
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
