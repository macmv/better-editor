//! Shell view backend: runs each command line as `zsh -c` and carries shell
//! state (cwd, env, aliases, functions, options, ...) between commands.

pub mod history;
pub mod state;
pub mod ui_cmd;
pub mod zsh;

pub use history::{History, HistoryEntry};
pub use state::{DumpDir, ShellState};
pub use ui_cmd::{UiCommand, parse_ui_command};
pub use zsh::{CommandSpec, PreparedRun, RunResult, Session};
