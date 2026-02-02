use std::collections::HashMap;

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
    $(#[$attrs])*
    pub struct $name {
      $(pub $field_ident: $field_type,)*
    }

    #[derive(serde::Deserialize)]
    #[serde(default, rename_all = "kebab-case")]
    #[derive(Default)]
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
  #[derive(Clone)]
  pub struct Settings {
    pub font:   FontSettings,
    pub editor: EditorSettings,
    pub layout: LayoutSettings,
  }
);

config!(
  #[partial = FontSettingsPartial]
  #[derive(Clone)]
  pub struct FontSettings {
    pub family: String,
    pub size:   f64,
  }
);

config!(
  #[partial = LayoutSettingsPartial]
  #[derive(Clone)]
  pub struct LayoutSettings {
    pub tab: Vec<TabSettings>,
  }
);

#[derive(Clone, serde::Deserialize)]
#[serde(tag = "pane", rename_all = "kebab-case")]
pub enum TabSettings {
  Split(SplitSettings),
  FileTree,
  Editor,
  Terminal,
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SplitSettings {
  pub axis:     Axis,
  pub percent:  Vec<f64>,
  pub active:   usize,
  pub children: Vec<TabSettings>,
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Axis {
  Horizontal,
  Vertical,
}

config!(
  #[partial = EditorSettingsPartial]
  #[derive(Clone)]
  pub struct EditorSettings {
    pub scroll_offset: u32,
    pub indent_width:  u32,
  }
);

impl Settings {
  pub fn load() -> Settings {
    let mut config = crate::Config::default_ref().settings.clone();

    if let Ok(data) = std::fs::read_to_string(crate::config_root().unwrap().join("config.toml")) {
      match toml::from_str::<ConfigDataPartial>(&data) {
        Ok(partial) => config.replace_with(partial),
        Err(e) => eprintln!("failed to parse config: {e}"), // TODO: User-visible error
      }
    }

    config
  }

  pub(crate) fn parse_default() -> Settings { parse_default_config().unwrap() }
}

fn parse_default_config() -> Result<Settings, toml::de::Error> {
  toml::from_str(include_str!("../default.toml"))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_config() {
    if let Err(e) = parse_default_config() {
      panic!("invalid default config:\n{e}");
    }
  }
}
