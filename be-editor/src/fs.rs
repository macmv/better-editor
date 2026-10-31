use std::{
  io,
  ops::Deref,
  path::{Path, PathBuf},
  rc::Rc,
};

use be_doc::Document;
use be_fs::{Fs, LocalFs, PendingRead};

use crate::{CursorOrLsp, EditorState};

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

/// A file that is being read. The editor keeps showing what it had until the
/// read finishes, so a reload doesn't flash.
pub(crate) struct PendingOpen {
  read:   PendingRead,
  /// Where to put the cursor once the document arrives.
  cursor: Option<CursorOrLsp>,
}

impl EditorState {
  /// Starts reading `path`. A local file has finished by the time this
  /// returns, and a failure to read it is returned. A file on another machine
  /// is picked up by `layout` when it arrives, and a failure to read it is
  /// shown in the status line.
  pub fn open(&mut self, path: &Path) -> io::Result<()> {
    if let Some(_current) = &self.file {
      // TODO: Confirm save
    }

    // Replacing the old read drops it, which cancels its result.
    self.pending_open = Some(PendingOpen { read: self.fs.begin_read(path), cursor: None });
    self.update_open()
  }

  /// True while a file is being read.
  pub fn loading(&self) -> bool { self.pending_open.is_some() }

  /// Moves the cursor once the file has loaded. If nothing is loading, this
  /// moves it right away.
  pub fn move_to_when_loaded(&mut self, cursor: CursorOrLsp) {
    match &mut self.pending_open {
      Some(pending) => pending.cursor = Some(cursor),
      None => {
        let cursor = cursor.into_cursor(&self.doc);
        self.move_to(cursor);
      }
    }
  }

  /// Applies a finished `open`, if there is one. Returns the error if the read
  /// failed.
  pub(crate) fn update_open(&mut self) -> io::Result<()> {
    let Some(pending) = &mut self.pending_open else { return Ok(()) };
    let Some(res) = pending.read.poll() else { return Ok(()) };
    let pending = self.pending_open.take().unwrap();

    let data = res?;
    let mtime = data.mtime;
    let path = data.path;
    let doc = Document::read_lossy(&mut io::Cursor::new(data.data))?;

    self.file = Some(OpenedFile { path, mtime, saved_history_position: 0, modified: false });
    self.doc = doc;
    self.history.clear();
    self.history_position = 0;
    self.damage_all = true;

    self.on_open_file();

    if let Some(cursor) = pending.cursor {
      let cursor = cursor.into_cursor(&self.doc);
      self.move_to(cursor);
    }
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

#[cfg(test)]
mod tests {
  use be_doc::{Column, Cursor, Line, VisualColumn};

  use super::*;

  fn temp_file(name: &str, contents: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("be-editor-test-{}-{name}", std::process::id()));
    std::fs::write(&path, contents).unwrap();
    path
  }

  #[test]
  fn local_open_has_finished_on_return() {
    let path = temp_file("open", "hello\nworld\n");

    let mut editor = EditorState::new();
    editor.open(&path).unwrap();
    assert!(!editor.loading());
    assert_eq!(editor.doc().to_string(), "hello\nworld\n");
    assert_eq!(editor.file(), Some(path.canonicalize().unwrap().as_path()));

    std::fs::remove_file(path).unwrap();
  }

  #[test]
  fn missing_file_fails_at_once() {
    let mut editor = EditorState::new();
    assert!(editor.open(Path::new("/does/not/exist")).is_err());
    assert!(!editor.loading());
  }

  /// An `Fs` whose reads finish only when told to.
  struct SlowFs(Rc<std::cell::RefCell<Option<io::Result<be_fs::FileData>>>>);

  impl Fs for SlowFs {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> { Ok(path.to_path_buf()) }
    fn metadata(&self, _: &Path) -> io::Result<be_fs::Metadata> { unimplemented!() }
    fn read(&self, _: &Path) -> io::Result<Vec<u8>> { unimplemented!() }
    fn write(&self, _: &Path, _: &[u8]) -> io::Result<()> { unimplemented!() }
    fn read_dir(&self, _: &Path) -> io::Result<Vec<be_fs::DirEntry>> { unimplemented!() }

    fn begin_read(&self, _: &Path) -> PendingRead {
      let slot = self.0.clone();
      PendingRead::new(move || slot.borrow_mut().take())
    }
  }

  #[test]
  fn slow_open_applies_in_layout_and_defers_the_cursor() {
    let slot = Rc::new(std::cell::RefCell::new(None));
    let mut editor = EditorState::from("old\n");
    editor.fs = EditorFs(Rc::new(SlowFs(slot.clone())));

    editor.open(Path::new("/remote/a.txt")).unwrap();
    editor.move_to_when_loaded(CursorOrLsp::Cursor(Cursor {
      line:          Line(1),
      column:        Column(2),
      target_column: VisualColumn(2),
    }));

    // The old text stays until the new text arrives.
    assert!(editor.loading());
    editor.layout();
    assert_eq!(editor.doc().to_string(), "old\n");

    *slot.borrow_mut() = Some(Ok(be_fs::FileData {
      path:  "/remote/a.txt".into(),
      mtime: 0,
      data:  b"hello\nworld\n".to_vec(),
    }));
    editor.layout();
    assert!(!editor.loading());
    assert_eq!(editor.doc().to_string(), "hello\nworld\n");
    assert_eq!(editor.cursor().line, Line(1));
    assert_eq!(editor.cursor().column, Column(2));
  }

  #[test]
  fn slow_open_failure_lands_in_the_status_line() {
    let slot = Rc::new(std::cell::RefCell::new(None));
    let mut editor = EditorState::new();
    editor.fs = EditorFs(Rc::new(SlowFs(slot.clone())));

    editor.open(Path::new("/remote/a.txt")).unwrap();
    *slot.borrow_mut() = Some(Err(io::ErrorKind::NotFound.into()));
    editor.layout();

    assert!(!editor.loading());
    assert!(editor.status().is_some_and(|s| !s.success));
  }
}
