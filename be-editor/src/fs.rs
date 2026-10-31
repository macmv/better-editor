use std::{
  io,
  ops::Deref,
  path::{Path, PathBuf},
  rc::Rc,
};

use be_doc::Document;
use be_fs::{Fs, LocalFs, PendingRead, PendingWrite};

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

/// A save that is on its way to disk. It holds the bytes as they were when the
/// save began, so the editor can keep changing the document meanwhile.
pub(crate) struct PendingSave {
  write:                  PendingWrite,
  /// What `saved_history_position` becomes once the write lands.
  saved_history_position: usize,
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

  /// Starts writing the document to its file. A local file has been written by
  /// the time this returns, and a failure to write it is returned. A file on
  /// another machine is finished by `layout` when the write is answered, and a
  /// failure to write it is shown in the status line.
  ///
  /// The document is snapshotted here, so it can be edited while the write is
  /// in flight. A second save replaces the first's bookkeeping, but the first
  /// write still lands, and the host applies them in order.
  pub fn save(&mut self) -> io::Result<()> {
    let Some(file) = &self.file else {
      return Err(io::Error::new(io::ErrorKind::NotFound, "no file open"));
    };

    let mut data = vec![];
    self.doc.write(&mut data)?;

    self.pending_save = Some(PendingSave {
      write:                  self.fs.begin_write(file.path(), data),
      saved_history_position: self.history.len() - self.history_position,
    });

    let res = self.update_save();
    if self.pending_save.is_some() {
      self.status = Some(crate::Status::for_success("saving..."));
    }
    res
  }

  /// True while a save is on its way to disk.
  pub fn saving(&self) -> bool { self.pending_save.is_some() }

  /// Finishes a `save`, if its write has been answered. Returns the error if
  /// the write failed.
  pub(crate) fn update_save(&mut self) -> io::Result<()> {
    let Some(pending) = &mut self.pending_save else { return Ok(()) };
    let Some(res) = pending.write.poll() else { return Ok(()) };
    let pending = self.pending_save.take().unwrap();

    let written = res?;
    let Some(file) = &mut self.file else { return Ok(()) };
    if written.prev_mtime > file.mtime {
      // TODO: Confirm popup
      warn!("file has been modified");
    }
    file.mtime = written.mtime;
    file.saved_history_position = pending.saved_history_position;
    let path = file.path().to_path_buf();

    self.lsp_notify_did_save();

    if let Some(send) = &self.send {
      send(crate::EditorEvent::SavedFile(path.clone()));
    }

    let shown = match &self.root {
      Some(root) => path.strip_prefix(root).unwrap_or(&path),
      None => &path,
    };
    self.status = Some(crate::Status::for_success(format!("{}: written", shown.display())));
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
    fn begin_write(&self, _: &Path, _: Vec<u8>) -> PendingWrite { unimplemented!() }
    fn begin_read_dir(&self, _: &Path) -> be_fs::PendingReadDir { unimplemented!() }

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
