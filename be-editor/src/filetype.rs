use crate::EditorState;

impl EditorState {
  pub(crate) fn detect_filetype(&mut self) {
    let Some(file) = &self.file else { return };

    self.filetype = self.config.borrow().language_for_filename(&file.path().display().to_string());
  }
}
