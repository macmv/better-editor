//! Runs command lines for the shell view: each line is passed to `sh -c` in its
//! own pty, and its output is collected by a terminal emulator.
//!
//! There is no state carried between lines (no cd, no env changes), and no
//! background jobs: a line is over when its `sh` exits.

mod job;
mod shell;

pub use job::{Job, JobId};
pub use shell::{Shell, ShellEvent};
