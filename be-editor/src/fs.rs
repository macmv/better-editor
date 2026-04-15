use std::{
  io,
  os::unix::fs::MetadataExt,
  path::{Path, PathBuf},
};

use be_doc::Document;

use crate::EditorState;

pub struct OpenedFile {
  path:  PathBuf,
  mtime: i64,

  /// This is a little dumb, but: history is built in reverse on the editor. So,
  /// while editting, the history says at zero, and undoing increases it. We
  /// want the opposite behavior: we want a stable index into history.
  ///
  /// So, to index into real editor history, use `history.len() -
  /// saved_history_position`.
  pub(crate) saved_history_position: usize,

  /// Set if the file was modified underneath the editor session.
  modified: bool,
}

impl EditorState {
  pub fn open(&mut self, path: &Path) -> io::Result<()> {
    let canon = path.canonicalize()?;

    if let Some(current) = &self.file
      && current.path != canon
    {
      // TODO: Confirm save
    }

    let (file, doc) = OpenedFile::open(&canon)?;
    self.file = Some(file);
    self.doc = doc;
    self.damage_all = true;

    self.on_open_file();

    Ok(())
  }

  pub fn save(&mut self) -> io::Result<()> {
    if let Some(file) = &mut self.file {
      file.saved_history_position = self.history.len() - self.history_position;
      file.save(&self.doc)?;
    } else {
      return Err(io::Error::new(io::ErrorKind::NotFound, "no file open"));
    }

    self.lsp_notify_did_save();

    Ok(())
  }

  pub fn on_file_changed(&mut self) {
    let unsaved = self.unsaved();
    if let Some(file) = &mut self.file {
      if unsaved {
        file.modified = true;
      } else {
        let p = file.path().to_path_buf();
        self.open(&p).unwrap();
      }
    }
  }

  pub fn modified(&self) -> bool { self.file.as_ref().is_some_and(|f| f.modified) }
}

impl OpenedFile {
  pub fn path(&self) -> &Path { &self.path }

  pub fn open(path: &Path) -> io::Result<(OpenedFile, Document)> {
    let path = path.canonicalize()?;
    let stat = path.metadata()?;

    let doc = Document::read(&path)?;
    let file = OpenedFile { path, mtime: stat.mtime(), saved_history_position: 0, modified: false };

    Ok((file, doc))
  }

  pub fn save(&mut self, doc: &Document) -> io::Result<()> {
    let stat = self.path.metadata()?;
    if stat.mtime() > self.mtime {
      // TODO: Confirm popup
      // return Err(io::Error::new(io::ErrorKind::Other, "file has been modified"));
      warn!("file has been modified");
    }

    let mut file = std::fs::OpenOptions::new().write(true).truncate(true).open(&self.path)?;
    doc.write(&mut file)?;
    drop(file);

    let stat = self.path.metadata()?;
    self.mtime = stat.mtime();
    Ok(())
  }
}
