//! Shell view backend: runs each command line as `zsh -c` and carries shell
//! state (cwd, env, aliases, functions, options, ...) between commands.

pub mod clock;
pub mod duration;
pub mod history;
pub mod job;
pub mod prompt;
pub mod shell;
pub mod state;
pub mod ui_cmd;
pub mod zsh;

pub use clock::format_local_time;
pub use duration::format_duration;
pub use history::{History, HistoryEntry};
pub use job::{Job, JobId, JobState};
pub use prompt::{GitHead, GitStatus, PromptContext};
pub use shell::{STARTUP_ID, Shell, ShellEvent, SubmitResult};
pub use state::{DumpDir, ShellState};
pub use ui_cmd::{UiCommand, parse_ui_command};
pub use zsh::{CommandSpec, PreparedRun, RunResult, Session};
