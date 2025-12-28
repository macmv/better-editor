use std::{io, path::PathBuf};

mod config;

pub use config::Config;

fn config_root() -> io::Result<PathBuf> {
  Ok(dirs::config_dir().ok_or(io::ErrorKind::NotFound)?.join("be"))
}
