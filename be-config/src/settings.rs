use crate::{keybind::Keybinds, parse::ParseResult};

use be_config_macros::Config;

#[derive(Default, Config, Clone)]
pub struct Settings {
  pub editor:   EditorSettings,
  pub ui:       UiSettings,
  pub terminal: TerminalSettings,
  pub layout:   LayoutSettings,
  pub keybinds: Keybinds,
}

#[derive(Default, Config, Clone)]
pub struct EditorSettings {
  pub font:          FontSettings,
  pub scroll_offset: u32,
  pub indent_width:  u32,
}

#[derive(Default, Config, Clone)]
pub struct UiSettings {
  pub font: FontSettings,
}

#[derive(Default, Config, Clone)]
pub struct TerminalSettings {
  pub font: FontSettings,
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

#[derive(Default, Debug, Clone, Config, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Axis {
  #[default]
  Horizontal,
  Vertical,
}

impl Settings {
  pub fn load() -> ParseResult<Settings> {
    let mut config = crate::Config::default_ref().settings.clone();

    let diagnostics = if let Ok(data) =
      std::fs::read_to_string(crate::config_root().unwrap().join("config.toml"))
    {
      crate::parse::parse_into(&mut config, &data)
    } else {
      vec![]
    };

    ParseResult { value: config, diagnostics }
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

  #[test]
  fn navigation_binds() {
    use be_input::{Action, Key, KeyStroke, Mode, Navigation};

    let key = |key| KeyStroke { key, sup: false, shift: false, control: false, alt: false };
    let nav = |k| {
      let settings = parse_default_config().value;
      settings.keybinds.parse(Mode::Insert, &[key(Key::Char('\\')), key(k)])
    };

    assert!(matches!(nav(Key::Tab), Ok(Action::Navigate { nav: Navigation::Zoom })));
    assert!(matches!(nav(Key::Char('z')), Ok(Action::Navigate { nav: Navigation::Workspaces })));
    assert!(matches!(
      nav(Key::Char(' ')),
      Ok(Action::Navigate { nav: Navigation::CommandPalette })
    ));
    assert!(matches!(nav(Key::Char('r')), Ok(Action::Navigate { nav: Navigation::LastPane })));
  }

  #[test]
  fn describe_binds() {
    use be_input::Navigation;

    let keybinds = parse_default_config().value.keybinds;

    assert_eq!(keybinds.describe(Navigation::CommandPalette).as_deref(), Some("\\ SP"));
    assert_eq!(keybinds.describe(Navigation::Zoom).as_deref(), Some("\\ TAB"));
    assert_eq!(keybinds.describe(Navigation::OpenSearch).as_deref(), Some("SP s"));
  }
}
