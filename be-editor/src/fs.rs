use std::{
  os::unix::fs::MetadataExt,
  path::{Path, PathBuf},
};

use be_doc::Document;

pub struct OpenedFile {
  path:  PathBuf,
  mtime: i64,
}

impl OpenedFile {
  pub fn open(path: &Path) -> (OpenedFile, Document) {
    let path = path.canonicalize().unwrap();
    let stat = path.metadata().unwrap();

    let doc = Document::read(&path).unwrap();
    let file = OpenedFile { path, mtime: stat.mtime() };

    (file, doc)
  }

  pub fn save(&self, doc: &Document) {
    let stat = self.path.metadata().unwrap();
    if stat.mtime() > self.mtime {
      panic!("file has been modified");
    }

    let mut file = std::fs::OpenOptions::new().write(true).truncate(true).open(&self.path).unwrap();
    doc.write(&mut file).unwrap();
  }
}
