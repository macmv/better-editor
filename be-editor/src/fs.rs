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

    self.on_open_file();

    Ok(())
  }

  pub fn save(&mut self) -> io::Result<()> {
    if let Some(file) = &self.file {
      file.save(&self.doc)
    } else {
      Err(io::Error::new(io::ErrorKind::NotFound, "no file open"))
    }
  }
}

impl OpenedFile {
  pub fn path(&self) -> &Path { &self.path }

  pub fn open(path: &Path) -> io::Result<(OpenedFile, Document)> {
    let path = path.canonicalize()?;
    let stat = path.metadata()?;

    let doc = Document::read(&path)?;
    let file = OpenedFile { path, mtime: stat.mtime() };

    Ok((file, doc))
  }

  pub fn save(&self, doc: &Document) -> io::Result<()> {
    let stat = self.path.metadata()?;
    if stat.mtime() > self.mtime {
      return Err(io::Error::new(io::ErrorKind::Other, "file has been modified"));
    }

    let mut file = std::fs::OpenOptions::new().write(true).truncate(true).open(&self.path)?;
    doc.write(&mut file)
  }
}
