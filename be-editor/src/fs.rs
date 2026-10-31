use std::{
  io,
  ops::Deref,
  path::{Path, PathBuf},
  rc::Rc,
};

use be_doc::Document;
use be_fs::{Fs, LocalFs};

use crate::EditorState;

/// The filesystem an editor reads and writes through. Defaults to the local
/// disk; a workspace on another machine swaps in its own.
#[derive(Clone)]
pub struct EditorFs(pub Rc<dyn Fs>);

impl Default for EditorFs {
  fn default() -> Self { EditorFs(Rc::new(LocalFs)) }
}

impl Deref for EditorFs {
  type Target = dyn Fs;

  fn deref(&self) -> &Self::Target { &*self.0 }
}

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
    let canon = self.fs.canonicalize(path)?;

    if let Some(current) = &self.file
      && current.path != canon
    {
      // TODO: Confirm save
    }

    let (file, doc) = OpenedFile::open(&*self.fs, &canon)?;
    self.file = Some(file);
    self.doc = doc;
    self.history.clear();
    self.history_position = 0;
    self.damage_all = true;

    self.on_open_file();

    Ok(())
  }

  pub fn save(&mut self) -> io::Result<()> {
    if let Some(file) = &mut self.file {
      file.save(&*self.fs, &self.doc)?;
      file.saved_history_position = self.history.len() - self.history_position;
    } else {
      return Err(io::Error::new(io::ErrorKind::NotFound, "no file open"));
    }

    self.lsp_notify_did_save();

    if let (Some(send), Some(file)) = (&self.send, &self.file) {
      send(crate::EditorEvent::SavedFile(file.path().to_path_buf()));
    }

    Ok(())
  }

  pub fn on_file_changed(&mut self) {
    let unsaved = self.unsaved();
    if let Some(file) = &mut self.file {
      if unsaved {
        file.modified = true;
      } else {
        let p = file.path().to_path_buf();
        if let Err(e) = self.open(&p) {
          warn!("failed to reload file on change: {e}");
          if let Some(file) = &mut self.file {
            file.modified = true;
          }
        }
      }
    }
  }

  pub fn modified(&self) -> bool { self.file.as_ref().is_some_and(|f| f.modified) }
}

impl OpenedFile {
  pub fn path(&self) -> &Path { &self.path }

  pub fn open(fs: &dyn Fs, path: &Path) -> io::Result<(OpenedFile, Document)> {
    let path = fs.canonicalize(path)?;
    let stat = fs.metadata(&path)?;

    let doc = Document::read_lossy(&mut io::Cursor::new(fs.read(&path)?))?;
    let file = OpenedFile { path, mtime: stat.mtime, saved_history_position: 0, modified: false };

    Ok((file, doc))
  }

  pub fn save(&mut self, fs: &dyn Fs, doc: &Document) -> io::Result<()> {
    let stat = fs.metadata(&self.path)?;
    if stat.mtime > self.mtime {
      // TODO: Confirm popup
      // return Err(io::Error::new(io::ErrorKind::Other, "file has been
      // modified"));
      warn!("file has been modified");
    }

    let mut data = vec![];
    doc.write(&mut data)?;
    fs.write(&self.path, &data)?;

    self.mtime = fs.metadata(&self.path)?.mtime;
    Ok(())
  }
}
