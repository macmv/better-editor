use std::collections::HashMap;

macro_rules! builtin {
  ($($filename:literal),* $(,)?) => {
    const BUILTIN_LANGUAGES: &[(&str, &str)] = &[
      $(
        ($filename, include_str!(concat!("./builtin/", $filename, ".toml"))),
      )*
    ];
  }
}

builtin!["markdown", "rust", "toml"];

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct LanguageName {
  name: &'static str,
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Language {
  pub display_name: String,
  pub extensions:   Vec<String>,
  pub tree_sitter:  Option<TreeSitterSettings>,
  pub lsp:          Option<LspSettings>,
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TreeSitterSettings {
  pub repo: String,
  pub path: Option<String>,
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LspSettings {
  pub command: String,
}

impl Language {
  pub fn builtin() -> HashMap<LanguageName, Language> {
    BUILTIN_LANGUAGES
      .iter()
      .map(|(name, content)| (LanguageName { name }, Language::parse(content).unwrap()))
      .collect()
  }

  pub fn parse(content: &str) -> Result<Language, toml::de::Error> {
    toml::from_str::<Language>(content)
  }
}

impl LanguageName {
  pub fn name(&self) -> &str { self.name }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn builtin_languages() {
    for (name, lang) in BUILTIN_LANGUAGES {
      if let Err(e) = Language::parse(lang) {
        panic!("invalid builtin language {name}:\n{e}");
      }
    }
  }
}
