use std::{io, path::PathBuf};

fn config_root() -> io::Result<PathBuf> {
  Ok(dirs::config_dir().ok_or(io::ErrorKind::NotFound)?.join("be"))
}
