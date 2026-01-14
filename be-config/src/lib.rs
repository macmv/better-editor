use std::{io, path::PathBuf};

mod config;

pub use config::*;

fn config_root() -> io::Result<PathBuf> {
  #[cfg(unix)]
  let base: PathBuf = std::env::home_dir().ok_or(io::ErrorKind::NotFound)?.join(".config");
  #[cfg(not(unix))]
  compile_error!("no config path set for target platform");

  Ok(base.join("be"))
}

pub fn cache_root() -> io::Result<PathBuf> {
  #[cfg(unix)]
  let base: PathBuf = std::env::home_dir().ok_or(io::ErrorKind::NotFound)?.join(".cache");
  #[cfg(not(unix))]
  compile_error!("no cache path set for target platform");

  Ok(base.join("be"))
}
