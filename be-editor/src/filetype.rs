use crate::EditorState;

impl EditorState {
  pub(crate) fn detect_filetype(&mut self) {
    let Some(file) = &self.file else { return };
    let Some(ext) = file.path().extension().and_then(|e| e.to_str()) else { return };

    for (&ft, language) in &self.config.borrow().languages {
      for extension in &language.extensions {
        if extension == ext {
          self.filetype = Some(ft);
          return;
        }
      }
    }

    self.filetype = None;
  }
}
