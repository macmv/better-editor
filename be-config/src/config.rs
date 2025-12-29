use std::{collections::HashMap, sync::LazyLock};

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
    #[serde(rename_all = "kebab-case")]
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
  pub struct Config {
    pub font:     FontSettings,
    pub language: HashMap<String, LanguageSettings>,
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

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LanguageSettings {
  pub tree_sitter: String,
  pub lsp:         LspSettings,
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LspSettings {
  pub command: String,
}

static DEFAULT_CONFIG: LazyLock<Config> = LazyLock::new(Config::parse_default);

impl Config {
  pub fn default() -> &'static Config { &*DEFAULT_CONFIG }

  pub fn load() -> Config {
    let mut config = Config::default().clone();

    if let Ok(data) = std::fs::read_to_string(crate::config_root().unwrap().join("config.toml")) {
      match toml::from_str::<ConfigDataPartial>(&data) {
        Ok(partial) => config.replace_with(partial),
        Err(e) => eprintln!("failed to parse config: {e}"), // TODO: User-visible error
      }
    }

    config
  }

  fn parse_default() -> Config { parse_default_config().unwrap() }
}

fn parse_default_config() -> Result<Config, toml::de::Error> {
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
