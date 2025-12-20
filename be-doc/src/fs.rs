use std::{
  io::{self, BufWriter, Write},
  path::Path,
};

use crop::RopeBuilder;

use crate::Document;

impl Document {
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
}

#[cfg(test)]
mod tests {
  use std::io::{self, Read};

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
