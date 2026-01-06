use std::{
  convert::Infallible,
  ops::Range,
  path::{Path, PathBuf},
  str::FromStr,
};

use be_doc::{Cursor, Document};
use be_task::Task;
use serde_json::value::RawValue;
use types::Uri;

use crate::{Diagnostic, LspClient, TextEdit, client::LspWorker};

pub trait LspCommand {
  type Result;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool;
  fn send(&self, client: &mut LspClient) -> Option<Task<Self::Result>>;
}

fn doc_uri(path: &Path) -> Uri {
  Uri::from_str(&format!("file://{}", path.to_string_lossy())).unwrap()
}

fn doc_id(path: &Path) -> types::TextDocumentIdentifier {
  types::TextDocumentIdentifier { uri: doc_uri(path) }
}

impl LspClient {
  pub fn encode_range(&self, doc: &Document, range: Range<usize>) -> types::Range {
    types::Range {
      start: self.encode_position(doc, range.start),
      end:   self.encode_position(doc, range.end),
    }
  }

  pub fn encode_position(&self, doc: &Document, pos: usize) -> types::Position {
    let line = doc.rope.line_of_byte(pos);

    let character = match self.position_encoding() {
      PositionEncoding::Utf8 => (pos - doc.byte_of_line(be_doc::Line(line))) as u32,
      PositionEncoding::Utf16 => doc
        .rope
        .byte_slice(pos - doc.byte_of_line(be_doc::Line(line))..pos)
        .chars()
        .map(|c| c.len_utf16())
        .sum::<usize>() as u32,
    };

    types::Position { line: line as u32, character }
  }

  pub fn encode_cursor(&self, doc: &Document, cursor: Cursor) -> types::Position {
    types::Position {
      line:      cursor.line.as_usize() as u32,
      character: {
        match self.position_encoding() {
          PositionEncoding::Utf8 => doc.cursor_column_offset(cursor) as u32,
          PositionEncoding::Utf16 => {
            let line = doc.line(cursor.line);
            line
              .graphemes()
              .take(cursor.column.0)
              .map(|g| g.chars().map(|c| c.len_utf16()).sum::<usize>())
              .sum::<usize>() as u32
          }
        }
      },
    }
  }

  pub fn position_encoding(&self) -> PositionEncoding { self.state.lock().position_encoding() }
}

pub fn decode_range(
  encoding: PositionEncoding,
  doc: &Document,
  range: types::Range,
) -> Range<usize> {
  Range {
    start: decode_position(encoding, doc, range.start),
    end:   decode_position(encoding, doc, range.end),
  }
}

pub fn decode_position(encoding: PositionEncoding, doc: &Document, pos: types::Position) -> usize {
  let character = match encoding {
    PositionEncoding::Utf8 => pos.character as usize,
    PositionEncoding::Utf16 => {
      let line = doc.line(be_doc::Line(pos.line as usize));
      let mut total = 0;
      let mut character = 0;
      for c in line.chars() {
        if total + c.len_utf16() > pos.character as usize {
          break;
        }
        total += c.len_utf16();
        character += c.len_utf8();
      }
      character
    }
  };

  doc.byte_of_line(be_doc::Line(pos.line as usize)) + character as usize
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PositionEncoding {
  Utf8,
  Utf16,
}

pub struct DidOpenTextDocument {
  pub path:        PathBuf,
  pub doc:         Document,
  pub language_id: String,
}

impl LspCommand for DidOpenTextDocument {
  type Result = Infallible;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.text_document_sync.is_some()
  }

  fn send(&self, client: &mut LspClient) -> Option<Task<Infallible>> {
    if client.state.lock().opened_files.insert(self.path.clone(), self.doc.clone()).is_some() {
      return None;
    }

    client.notify::<types::notification::DidOpenTextDocument>(types::DidOpenTextDocumentParams {
      text_document: types::TextDocumentItem {
        version:     0,
        uri:         doc_uri(&self.path),
        text:        self.doc.rope.to_string(),
        language_id: self.language_id.clone(),
      },
    });

    None
  }
}

pub struct DidChangeTextDocument {
  pub path:    PathBuf,
  pub version: i32,
  pub doc:     Document,
  pub changes: Vec<(Range<usize>, String)>,
}

impl LspCommand for DidChangeTextDocument {
  type Result = Infallible;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.text_document_sync.is_some()
  }

  fn send(&self, client: &mut LspClient) -> Option<Task<Self::Result>> {
    if !client.state.lock().opened_files.contains_key(&self.path) {
      error!("cannot change a file that is not opened: {}", self.path.display());
      return None;
    }

    client.notify::<types::notification::DidChangeTextDocument>(
      types::DidChangeTextDocumentParams {
        text_document:   types::VersionedTextDocumentIdentifier {
          uri:     doc_uri(&self.path),
          version: self.version,
        },
        content_changes: self
          .changes
          .iter()
          .map(|change| types::TextDocumentContentChangeEvent {
            range:        Some(client.encode_range(&self.doc, change.0.clone())),
            range_length: None,
            text:         change.1.clone(),
          })
          .collect(),
      },
    );

    None
  }
}

pub struct Completion {
  pub path:   PathBuf,
  pub doc:    Document,
  pub cursor: Cursor,
}

impl LspCommand for Completion {
  type Result = Option<types::CompletionResponse>;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.completion_provider.is_some()
  }

  fn send(&self, client: &mut LspClient) -> Option<Task<Option<types::CompletionResponse>>> {
    Some(client.request::<types::request::Completion>(types::CompletionParams {
      text_document_position:    types::TextDocumentPositionParams {
        text_document: doc_id(&self.path),
        position:      client.encode_cursor(&self.doc, self.cursor),
      },
      context:                   None,
      work_done_progress_params: types::WorkDoneProgressParams::default(),
      partial_result_params:     types::PartialResultParams::default(),
    }))
  }
}

pub struct DocumentFormat {
  pub path: PathBuf,
  pub doc:  Document,
}

impl LspCommand for DocumentFormat {
  type Result = Vec<TextEdit>;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.document_formatting_provider.is_some()
  }

  fn send(&self, client: &mut LspClient) -> Option<Task<Vec<TextEdit>>> {
    let encoding = client.position_encoding();
    let doc = self.doc.clone();

    Some(
      client
        .request::<types::request::Formatting>(types::DocumentFormattingParams {
          text_document:             doc_id(&self.path),
          options:                   types::FormattingOptions {
            tab_size: 2,
            insert_spaces: true,
            ..Default::default()
          },
          work_done_progress_params: types::WorkDoneProgressParams::default(),
        })
        .map(move |r| {
          if let Some(res) = r {
            res
              .into_iter()
              .map(|edit| TextEdit {
                range:    decode_range(encoding, &doc, edit.range),
                new_text: edit.new_text,
              })
              .collect()
          } else {
            vec![]
          }
        }),
    )
  }
}

impl LspWorker {
  pub fn handle_notification(&self, method: &str, params: Option<Box<RawValue>>) {
    if method == "textDocument/publishDiagnostics" {
      let params =
        serde_json::from_str::<lsp_types::PublishDiagnosticsParams>(params.unwrap().get()).unwrap();

      let encoding = self.state.lock().position_encoding();
      let path = PathBuf::from(params.uri.path().as_str());
      let diagnostics = params
        .diagnostics
        .into_iter()
        .map(|d| Diagnostic {
          range:    decode_range(encoding, &self.state.lock().opened_files[&path], d.range),
          severity: d.severity,
          message:  d.message,
        })
        .collect();

      self.state.lock().diagnostics.insert(path, diagnostics);
    } else {
      info!("unhandled notification: {}", method);
    }
  }
}
