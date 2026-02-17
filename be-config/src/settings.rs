use crate::parse::ParseResult;

use be_config_macros::Config;

#[derive(Default, Config, Clone)]
pub struct Settings {
  pub font:   FontSettings,
  pub editor: EditorSettings,
  pub layout: LayoutSettings,
}

#[derive(Default, Config, Clone)]
pub struct FontSettings {
  pub family: String,
  pub size:   f64,
}

#[derive(Default, Config, Clone)]
pub struct LayoutSettings {
  pub tab: Vec<TabSettings>,
}

#[derive(Default, Clone, Config, serde::Deserialize)]
#[config(tag = "pane")]
#[serde(tag = "pane", rename_all = "kebab-case")]
pub enum TabSettings {
  Split(SplitSettings),
  FileTree,
  Editor,
  #[default]
  Terminal,
}

#[derive(Default, Clone, Config, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SplitSettings {
  pub axis:     Axis,
  pub percent:  Vec<f64>,
  pub active:   usize,
  pub children: Vec<TabSettings>,
}

#[derive(Default, Clone, Config, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Axis {
  #[default]
  Horizontal,
  Vertical,
}

#[derive(Default, Config, Clone)]
pub struct EditorSettings {
  pub scroll_offset: u32,
  pub indent_width:  u32,
}

impl Settings {
  pub fn load() -> ParseResult<Settings> {
    let mut config = crate::Config::default_ref().settings.clone();

    if let Ok(data) = std::fs::read_to_string(crate::config_root().unwrap().join("config.toml")) {
      let diagnostics = crate::parse::parse_into(&mut config, &data);

      for diag in diagnostics {
        eprintln!("failed to parse config: {diag}"); // TODO: User-visible error
      }
    }

    ParseResult::ok(config)
  }

  pub(crate) fn parse_default() -> Settings { parse_default_config().value }
}

fn parse_default_config() -> ParseResult<Settings> {
  crate::parse::parse(include_str!("../default.toml"))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_config() {
    let res = parse_default_config();
    if !res.diagnostics.is_empty() {
      panic!(
        "invalid default config:\n{}",
        res.diagnostics.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
      );
    }
  }
}
