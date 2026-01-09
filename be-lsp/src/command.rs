use std::{
  convert::Infallible,
  ops::Range,
  path::{Path, PathBuf},
};

use be_doc::{Change, Cursor, Document};
use be_task::Task;
use serde_json::value::RawValue;

use crate::{
  Diagnostic, LspClient, Progress, TextEdit,
  client::{FileState, LspState, LspWorker},
};

pub trait LspCommand {
  type Result;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool;
  fn send(&self, client: &mut LspClient) -> Option<Task<Self::Result>>;
}

fn doc_uri(path: &Path) -> types::Uri { types::Uri::from_file_path(path) }

fn doc_id(path: &Path) -> types::TextDocumentIdentifier {
  types::TextDocumentIdentifier { uri: doc_uri(path) }
}

impl LspState {
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

  pub fn position_encoding(&self) -> PositionEncoding {
    match self.caps.position_encoding {
      Some(lsp::PositionEncodingKind::Utf8) => PositionEncoding::Utf8,
      _ => PositionEncoding::Utf16,
    }
  }

  pub fn file(&self, path: &PathBuf) -> Option<&FileState> {
    if let Some(f) = self.files.get(path) {
      Some(f)
    } else {
      error!("file not opened: {}", path.display());
      None
    }
  }

  pub fn file_mut(&mut self, path: &PathBuf) -> Option<&mut FileState> {
    if let Some(f) = self.files.get_mut(path) {
      Some(f)
    } else {
      error!("file not opened: {}", path.display());
      None
    }
  }
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
    if client
      .state
      .lock()
      .files
      .insert(self.path.clone(), FileState::from(self.doc.clone()))
      .is_some()
    {
      error!("file already opened: {}", self.path.display());
      return None;
    }

    client.notify::<types::notification::TextDocumentDidOpen>(types::DidOpenTextDocumentParams {
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
  pub path:              PathBuf,
  pub version:           i32,
  pub doc_before_change: Document,
  pub changes:           Vec<Change>,
}

impl LspCommand for DidChangeTextDocument {
  type Result = Infallible;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.text_document_sync.is_some()
  }

  fn send(&self, client: &mut LspClient) -> Option<Task<Self::Result>> {
    let content_changes = {
      let mut state = client.state.lock();
      let file = state.file_mut(&self.path)?;
      file.doc = self.doc_before_change.clone();
      for change in &self.changes {
        file.doc.apply(change);
      }

      self
        .changes
        .iter()
        .map(|change| {
          types::TextDocumentContentChangeEvent::RangeRangeRangeLengthUintegerTextString {
            range:        state.encode_range(&self.doc_before_change, change.range.clone()),
            range_length: None,
            text:         change.text.clone(),
          }
        })
        .collect()
    };

    client.notify::<types::notification::TextDocumentDidChange>(
      types::DidChangeTextDocumentParams {
        text_document: types::VersionedTextDocumentIdentifier {
          text_document_identifier: doc_id(&self.path),
          version:                  self.version,
        },
        content_changes,
      },
    );

    None
  }
}

pub struct DidSaveTextDocument {
  pub path: PathBuf,
}

impl LspCommand for DidSaveTextDocument {
  type Result = Infallible;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.text_document_sync.is_some()
  }

  fn send(&self, client: &mut LspClient) -> Option<Task<Infallible>> {
    client.notify::<types::notification::TextDocumentDidSave>(types::DidSaveTextDocumentParams {
      text_document: doc_id(&self.path),
      text:          None,
    });
    None
  }
}

pub struct Completion {
  pub path:   PathBuf,
  pub cursor: Cursor,
}

impl LspCommand for Completion {
  type Result = Option<types::Or2<Vec<types::CompletionItem>, types::CompletionList>>;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.completion_provider.is_some()
  }

  fn send(
    &self,
    client: &mut LspClient,
  ) -> Option<Task<Option<types::Or2<Vec<types::CompletionItem>, types::CompletionList>>>> {
    let position = {
      let state = client.state.lock();
      let file = state.file(&self.path)?;
      state.encode_cursor(&file.doc, self.cursor)
    };

    Some(client.request::<types::request::TextDocumentCompletion>(types::CompletionParams {
      text_document_position_params: types::TextDocumentPositionParams {
        text_document: doc_id(&self.path),
        position,
      },
      context:                       None,
      work_done_progress_params:     types::WorkDoneProgressParams::default(),
      partial_result_params:         types::PartialResultParams::default(),
    }))
  }
}

pub struct DocumentFormat {
  pub path: PathBuf,
}

impl LspCommand for DocumentFormat {
  type Result = Vec<TextEdit>;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.document_formatting_provider.is_some()
  }

  fn send(&self, client: &mut LspClient) -> Option<Task<Vec<TextEdit>>> {
    let (encoding, doc) = {
      let state = client.state.lock();
      (state.position_encoding(), state.file(&self.path)?.doc.clone())
    };

    Some(
      client
        .request::<types::request::TextDocumentFormatting>(types::DocumentFormattingParams {
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
  pub fn handle_request(
    &self,
    method: &str,
    params: Option<Box<RawValue>>,
  ) -> Option<Box<RawValue>> {
    struct Requester<'a> {
      worker: &'a LspWorker,
      method: &'a str,
      params: Option<Box<RawValue>>,
      result: Option<Box<RawValue>>,
    }

    impl Requester<'_> {
      fn on<R: lsp::request::Request>(
        &mut self,
        f: fn(&mut LspState, R::Params) -> R::Result,
      ) -> &mut Self {
        if self.method == R::METHOD {
          let params =
            serde_json::from_str::<R::Params>(self.params.as_ref().unwrap().get()).unwrap();

          let result = f(&mut self.worker.state.lock(), params);

          self.result =
            Some(RawValue::from_string(serde_json::to_string(&result).unwrap()).unwrap());
        }

        self
      }
    }

    let mut req = Requester { worker: self, method, params, result: None };

    req.on::<types::request::WindowWorkDoneProgressCreate>(on_work_done_progress_create);

    if req.result.is_none() {
      info!("unhandled request: {}", method);
    }

    req.result
  }

  pub fn handle_notification(&self, method: &str, params: Option<Box<RawValue>>) {
    struct Notifier<'a> {
      worker:  &'a LspWorker,
      method:  &'a str,
      params:  Option<Box<RawValue>>,
      handled: bool,
    }

    impl Notifier<'_> {
      fn on<R: lsp::notification::Notification>(
        &mut self,
        f: fn(&mut LspState, R::Params),
      ) -> &mut Self {
        if self.method == R::METHOD {
          let params =
            serde_json::from_str::<R::Params>(self.params.as_ref().unwrap().get()).unwrap();

          f(&mut self.worker.state.lock(), params);

          self.handled = true;
        }

        self
      }
    }

    let mut req = Notifier { worker: self, method, params, handled: false };

    req
      .on::<types::notification::TextDocumentPublishDiagnostics>(on_publish_diagnostics)
      .on::<types::notification::Progress>(on_progress);

    if !req.handled {
      info!("unhandled notification: {}", method);
    }
  }
}

fn on_work_done_progress_create(state: &mut LspState, params: lsp::WorkDoneProgressCreateParams) {
  let token = match params.token {
    lsp::ProgressToken::Integer(n) => n.to_string(),
    lsp::ProgressToken::String(s) => s,
  };

  state
    .progress
    .insert(token, Progress { title: "".into(), message: None, progress: 0.0, completed: None });
}

fn on_publish_diagnostics(state: &mut LspState, params: lsp::PublishDiagnosticsParams) {
  let path = params.uri.to_file_path().unwrap();

  let encoding = state.position_encoding();
  let Some(file) = state.file_mut(&path) else { return };

  file.diagnostics.clear();
  file.diagnostics.extend(params.diagnostics.into_iter().map(|d| Diagnostic {
    range:    decode_range(encoding, &file.doc, d.range),
    severity: d.severity,
    message:  d.message,
  }));
}

fn on_progress(state: &mut LspState, params: lsp::ProgressParams) {
  let work = serde_json::from_str::<lsp::WorkDoneProgress>(params.value.get()).unwrap();

  let token = match params.token {
    lsp::ProgressToken::Integer(n) => n.to_string(),
    lsp::ProgressToken::String(s) => s,
  };

  match work {
    lsp::WorkDoneProgress::Begin(begin) => {
      if !state.progress.contains_key(&token) {
        warn!("work done for unknown token: {}", token);
      }

      state.progress.insert(
        token,
        Progress {
          title:     begin.title,
          message:   begin.message,
          progress:  0.0,
          completed: None,
        },
      );
    }
    lsp::WorkDoneProgress::Report(report) => {
      if let Some(progress) = state.progress.get_mut(&token) {
        progress.message = report.message;
        progress.progress = report.percentage.unwrap_or(0) as f64 / 100.0;
      }
    }
    lsp::WorkDoneProgress::End(_) => {
      if let Some(progress) = state.progress.get_mut(&token) {
        progress.completed = Some(std::time::Instant::now());
        progress.progress = 1.0;
      }
    }
  }
}
