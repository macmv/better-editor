use std::collections::HashMap;

use crate::parse::ParseResult;

use be_config_macros::Config;

trait Partial {
  type Partial;
  fn replace_with(&mut self, partial: Self::Partial);
}

macro_rules! partial_option {
  ($ty:ty) => {
    impl Partial for $ty {
      type Partial = Option<$ty>;
      fn replace_with(&mut self, partial: Option<Self>) {
        if let Some(partial) = partial {
          *self = partial;
        }
      }
    }
  };
}

partial_option!(String);
partial_option!(f64);
partial_option!(u32);

impl<T> Partial for HashMap<String, T> {
  type Partial = Option<HashMap<String, T>>;

  fn replace_with(&mut self, partial: Self::Partial) {
    if let Some(partial) = partial {
      for (key, value) in partial {
        self.insert(key, value);
      }
    }
  }
}

impl<T> Partial for Vec<T> {
  type Partial = Option<Vec<T>>;

  fn replace_with(&mut self, partial: Self::Partial) {
    if let Some(partial) = partial {
      *self = partial;
    }
  }
}

macro_rules! config {
  (
    #[partial = $partial_name:ident]
    $(#[$attrs:meta])*
    pub struct $name:ident {
      $(
        pub $field_ident:ident: $field_type:ty,
      )*
    }
  ) => {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "kebab-case")]
    #[derive(Config)]
    $(#[$attrs])*
    pub struct $name {
      $(pub $field_ident: $field_type,)*
    }

    #[derive(serde::Deserialize)]
    #[serde(default, rename_all = "kebab-case")]
    $(#[$attrs])*
    struct $partial_name {
      $($field_ident: <$field_type as Partial>::Partial,)*
    }

    impl Partial for $name {
      type Partial = $partial_name;

      fn replace_with(&mut self, partial: $partial_name) {
        $(
          self.$field_ident.replace_with(partial.$field_ident);
        )*
      }
    }
  };
}

config!(
  #[partial = ConfigDataPartial]
  #[derive(Default, Clone)]
  pub struct Settings {
    pub font:   FontSettings,
    pub editor: EditorSettings,
    pub layout: LayoutSettings,
  }
);

config!(
  #[partial = FontSettingsPartial]
  #[derive(Default, Clone)]
  pub struct FontSettings {
    pub family: String,
    pub size:   f64,
  }
);

config!(
  #[partial = LayoutSettingsPartial]
  #[derive(Default, Clone)]
  pub struct LayoutSettings {
    pub tab: Vec<TabSettings>,
  }
);

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

config!(
  #[partial = EditorSettingsPartial]
  #[derive(Default, Clone)]
  pub struct EditorSettings {
    pub scroll_offset: u32,
    pub indent_width:  u32,
  }
);

impl Settings {
  pub fn load() -> ParseResult<Settings> {
    let mut config = crate::Config::default_ref().settings.clone();

    if let Ok(data) = std::fs::read_to_string(crate::config_root().unwrap().join("config.toml")) {
      match toml::from_str::<ConfigDataPartial>(&data) {
        Ok(partial) => config.replace_with(partial),
        Err(e) => eprintln!("failed to parse config: {e}"), // TODO: User-visible error
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
