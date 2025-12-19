use std::{
  io::{self, BufWriter, Write},
  ops::Add,
  path::Path,
};

use crop::{Rope, RopeBuilder, RopeSlice};

#[derive(Default)]
pub struct Document {
  pub rope: Rope,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor {
  pub line:          Line,
  pub column:        Column,
  pub target_column: Column,
}

/// A visual line, ie, lines from the start of the file.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Line(pub usize);

/// A visual column, ie, graphemes from the start of the line.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Column(pub usize);

impl From<&str> for Document {
  fn from(s: &str) -> Document { Document { rope: Rope::from(s) } }
}

impl Cursor {
  pub const START: Cursor =
    Cursor { line: Line(0), column: Column(0), target_column: Column(0) };
}

impl Default for Cursor {
  fn default() -> Self { Cursor::START }
}

impl PartialEq<usize> for Line {
  fn eq(&self, other: &usize) -> bool { self.0 == *other }
}
impl PartialEq<usize> for Column {
  fn eq(&self, other: &usize) -> bool { self.0 == *other }
}

impl Document {
  pub fn new() -> Document { Document { rope: Rope::new() } }
  pub fn read_lossy(reader: &mut impl std::io::Read) -> io::Result<Document> {
    let mut builder = RopeBuilder::new();

    let mut chunk = [0_u8; 1024];
    let mut start = 0;
    loop {
      let n = reader.read(&mut chunk[start..]).unwrap();
      if n == 0 {
        break;
      }
      let mut remaining = start + n;

      while remaining > 0 {
        match str::from_utf8(&chunk[..remaining]) {
          Ok(s) => {
            builder.append(s);
            start = 0;
            break;
          }
          Err(e) => {
            let valid_bytes = e.valid_up_to();
            builder.append(str::from_utf8(&chunk[..valid_bytes]).unwrap());

            match e.error_len() {
              None => {
                chunk.copy_within(valid_bytes..remaining, 0);
                start = remaining - valid_bytes;
                break;
              }

              Some(len) => {
                chunk.copy_within(valid_bytes + len..remaining, 0);
                remaining -= valid_bytes + len;
                builder.append("\u{FFFD}");
              }
            }
          }
        }
      }
    }

    Ok(Document { rope: builder.build() })
  }

  pub fn write(&self, writer: &mut impl std::io::Write) -> io::Result<()> {
    let mut writer = BufWriter::new(writer);

    for chunk in self.rope.chunks() {
      writer.write_all(chunk.as_bytes())?;
    }

    Ok(())
  }

  pub fn read(path: &Path) -> io::Result<Document> {
    Document::read_lossy(&mut std::fs::File::open(path)?)
  }

  pub fn line(&self, line: Line) -> RopeSlice<'_> { self.rope.line(line.0) }
  pub fn len_lines(&self) -> usize { self.rope.line_len() }
}

impl Column {
  pub fn as_usize(&self) -> usize { self.0 }
  pub fn clamp(self, max: Column) -> Column { Column(self.0.clamp(0, max.0)) }
}

impl Line {
  pub fn as_usize(&self) -> usize { self.0 }
  pub fn clamp(self, max: Line) -> Line { Line(self.0.clamp(0, max.0)) }
}

impl Add<i32> for Column {
  type Output = Column;

  fn add(self, rhs: i32) -> Column { Column((self.0 as isize + rhs as isize).max(0) as usize) }
}

impl Add<i32> for Line {
  type Output = Line;

  fn add(self, rhs: i32) -> Line { Line((self.0 as isize + rhs as isize).max(0) as usize) }
}

#[cfg(test)]
mod tests {
  use std::io::Read;

  use super::*;

  #[test]
  fn doc_read_lossy() {
    let doc = Document::read_lossy(&mut std::io::Cursor::new([b'a', 150, b'b', b'c'])).unwrap();
    assert_eq!(doc.rope, "a\u{FFFD}bc");
  }

  struct ReadIn2<T>(T);

  impl<T: Read> Read for ReadIn2<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
      let len = 2.min(buf.len());
      self.0.read(&mut buf[..len])
    }
  }

  #[test]
  fn doc_read_emoji() {
    let doc = Document::read_lossy(&mut std::io::Cursor::new([0xf0, 0x9f, 0x92, 0x96])).unwrap();
    assert_eq!(doc.rope, "💖");
  }

  #[test]
  fn doc_read_across_chunks() {
    let doc =
      Document::read_lossy(&mut ReadIn2(std::io::Cursor::new([0xf0, 0x9f, 0x92, 0x96]))).unwrap();
    assert_eq!(doc.rope, "💖");
  }
}
