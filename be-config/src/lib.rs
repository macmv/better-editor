use std::{collections::HashMap, io, path::PathBuf, sync::LazyLock};

mod lang;
mod parse;
mod settings;

pub use lang::*;
pub use settings::*;

use crate::parse::ParseResult;

extern crate self as be_config;

#[derive(Clone)]
pub struct Config {
  pub settings:  Settings,
  pub languages: HashMap<LanguageName, Language>,
}

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

static DEFAULT_CONFIG: LazyLock<Config> = LazyLock::new(Config::load_default);

impl Default for Config {
  fn default() -> Self { Config::default_ref().clone() }
}

impl Config {
  pub fn default_ref() -> &'static Config { &*DEFAULT_CONFIG }

  pub fn load() -> ParseResult<Self> {
    Settings::load().map(|settings| Config { settings, languages: Language::builtin() })
  }

  fn load_default() -> Config {
    Config { settings: Settings::parse_default(), languages: Language::builtin() }
  }
}
