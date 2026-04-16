use be_doc::Document;
use be_task::Task;
use parking_lot::Mutex;
use polling::{Events, Poller};
use serde::de;
use serde_json::value::RawValue;
use std::{
  collections::{HashMap, VecDeque},
  fmt,
  io::{self, Read, Write},
  mem::ManuallyDrop,
  path::PathBuf,
  process::{Child, ChildStdin, ChildStdout, Stdio},
  sync::Arc,
};

use crate::{Diagnostic, Progress};

pub struct LspClient {
  _child:        Child,
  worker_thread: ManuallyDrop<std::thread::JoinHandle<()>>,
  next_id:       u64,

  poller: Arc<Poller>,
  tx:     ManuallyDrop<crossbeam_channel::Sender<LspRequest>>,

  pub state: Arc<Mutex<LspState>>,
}

/// This is all the state we've sent to a particular server.
#[derive(Default)]
pub struct LspState {
  pub caps:     types::ServerCapabilities,
  pub files:    HashMap<PathBuf, FileState>,
  pub progress: HashMap<String, Progress>,
}

#[derive(Default)]
pub struct FileState {
  pub version:     u32,
  pub doc:         Document,
  pub diagnostics: Vec<Diagnostic>,
}

impl From<Document> for FileState {
  fn from(value: Document) -> Self { FileState { doc: value, ..Default::default() } }
}

enum LspRequest {
  Request(Request, Completer),
  Notification(Notification),
}

struct Request {
  id:     u64,
  method: &'static str,
  params: Box<RawValue>,
}

struct Notification {
  method: &'static str,
  params: Box<RawValue>,
}

pub(crate) struct LspWorker {
  rx: crossbeam_channel::Receiver<LspRequest>,

  poller: Arc<Poller>,
  writer: Writer,
  reader: Reader,

  pub(crate) state: Arc<Mutex<LspState>>,

  pending: HashMap<u64, Completer>,

  pub on_message: Arc<Mutex<Box<dyn Fn() + Send>>>,
}

type Completer = Box<dyn FnOnce(&RawValue) + Send>;

struct Writer {
  writer: ChildStdin,
}

struct Reader {
  reader: ChildStdout,
  read:   VecDeque<u8>,
}

impl LspClient {
  pub fn spawn(
    cmd: &str,
    on_message: Arc<Mutex<Box<dyn Fn() + Send>>>,
  ) -> (LspClient, lsp::ServerCapabilities) {
    let mut child =
      std::process::Command::new(cmd).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();

    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    let (send_tx, send_rx) = crossbeam_channel::unbounded();

    let state = Arc::new(Mutex::new(LspState::default()));

    let worker = LspWorker {
      rx:      send_rx,
      state:   state.clone(),
      poller:  Arc::new(Poller::new().unwrap()),
      writer:  Writer::new(stdin),
      reader:  Reader::new(stdout),
      pending: HashMap::new(),

      on_message: on_message.clone(),
    };
    let poller = worker.poller.clone();
    let worker_thread = std::thread::spawn(move || worker.run());

    let mut client = LspClient {
      _child: child,
      worker_thread: ManuallyDrop::new(worker_thread),
      next_id: 1,
      state,
      poller,
      tx: ManuallyDrop::new(send_tx),
    };

    let init = lsp::InitializeParams {
      process_id: Some(std::process::id() as i32),
      capabilities: crate::init::client_capabilities(),
      ..Default::default()
    };

    let task = client.request::<lsp::request::Initialize>(init);

    let result = loop {
      match task.completed() {
        Some(msg) => break msg,
        None => {
          std::thread::sleep(std::time::Duration::from_millis(1));
        }
      }
    };

    client.state.lock().caps = result.capabilities.clone();

    client.notify::<lsp::notification::Initialized>(lsp::InitializedParams {});

    (client, result.capabilities)
  }

  pub fn request<T: lsp::request::Request>(&mut self, req: T::Params) -> Task<T::Result> {
    let task = Task::new();

    let completer = task.completer();
    let msg = LspRequest::Request(
      Request {
        id:     self.next_id,
        method: T::METHOD,
        params: RawValue::from_string(serde_json::to_string(&req).expect("serialize request"))
          .expect("valid json"),
      },
      Box::new(move |value| {
        let result = match serde_json::from_str(&value.get()) {
          Ok(r) => r,
          Err(e) => {
            error!("failed to deserialize LSP response for {}: {}", T::METHOD, e);
            return;
          }
        };
        match completer.complete(result) {
          Ok(()) => {}
          Err(_) => {}
        }
      }),
    );

    if let Err(e) = self.tx.send(msg) {
      error!("LSP worker is dead, dropping request {}: {}", T::METHOD, e);
    } else if let Err(e) = self.poller.notify() {
      error!("LSP poller notify failed: {}", e);
    }

    self.next_id += 1;

    task
  }

  pub fn notify<T: lsp::notification::Notification>(&mut self, req: T::Params) {
    let msg = LspRequest::Notification(Notification {
      method: T::METHOD,
      params: RawValue::from_string(serde_json::to_string(&req).expect("serialize notification"))
        .expect("valid json"),
    });

    if let Err(e) = self.tx.send(msg) {
      error!("LSP worker is dead, dropping notification {}: {}", T::METHOD, e);
    } else if let Err(e) = self.poller.notify() {
      error!("LSP poller notify failed: {}", e);
    }
  }

  pub fn shutdown(mut self) {
    unsafe {
      self.shutdown_mut();
    }
  }

  /// # Safety
  ///
  /// Must only be called once.
  pub unsafe fn shutdown_mut(&mut self) {
    self.notify::<lsp::notification::Exit>(());
    unsafe {
      ManuallyDrop::drop(&mut self.tx);
    }

    let thread = unsafe { ManuallyDrop::take(&mut self.worker_thread) };
    if let Err(e) = thread.join() {
      error!("LSP worker thread panicked on shutdown: {:?}", e);
    }
  }
}

impl LspWorker {
  pub fn run(mut self) {
    if let Err(e) = self.run_inner() {
      error!("LSP worker exited with error: {}", e);
    }
  }

  fn run_inner(&mut self) -> io::Result<()> {
    const READ: usize = 0;
    const WRITE: usize = 1;

    be_async::set_nonblocking(&self.reader.reader)?;
    be_async::set_nonblocking(&self.writer.writer)?;

    // SAFETY: These are removed down below.
    unsafe {
      self
        .poller
        .add_with_mode(&self.reader.reader, polling::Event::readable(READ), polling::PollMode::Edge)
        .unwrap();
      self
        .poller
        .add_with_mode(
          &self.writer.writer,
          polling::Event::writable(WRITE),
          polling::PollMode::Edge,
        )
        .unwrap();
    }

    'outer: loop {
      let mut events = Events::new();

      self.poller.wait(&mut events, Some(std::time::Duration::from_millis(10000)))?;
      for ev in events.iter() {
        match ev.key {
          READ => loop {
            match self.reader.recv() {
              Ok(Some(msg)) => {
                match msg {
                  Message::Request { id, method, params } => {
                    let res = self.handle_request(&method, params);
                    if let Some(res) = res {
                      self.writer.response(id, &res)?;
                    }
                  }
                  Message::Notification { method, params } => {
                    self.handle_notification(&method, params);
                  }
                  Message::Response { id, result, .. } => {
                    if let Some(completer) = self.pending.remove(&id) {
                      completer(&result);
                    }
                  }
                  Message::Error { id, .. } => {
                    if let Some(_) = self.pending.remove(&id) {
                      warn!("LSP error response for request {}", id);
                    }
                  }
                }

                self.on_message.lock()();
              }
              Ok(None) => break,
              Err(e) => {
                error!("LSP connection error: {}", e);
                break 'outer;
              }
            }
          },
          WRITE => {
            // TODO
          }

          _ => {
            warn!("unexpected polling event key: {}", ev.key);
          }
        }
      }

      loop {
        match self.rx.try_recv() {
          Ok(LspRequest::Request(req, completer)) => {
            self.pending.insert(req.id, completer);
            self.writer.request(req)?;
          }
          Ok(LspRequest::Notification(req)) => self.writer.notify(req)?,
          Err(crossbeam_channel::TryRecvError::Empty) => break,
          Err(crossbeam_channel::TryRecvError::Disconnected) => break 'outer,
        }
      }
    }

    let _ = self.poller.delete(&self.reader.reader);
    let _ = self.poller.delete(&self.writer.writer);

    Ok(())
  }
}

impl Writer {
  fn new(stdin: ChildStdin) -> Writer { Writer { writer: stdin } }

  fn request(&mut self, request: Request) -> io::Result<()> {
    #[derive(serde::Serialize)]
    struct Request {
      jsonrpc: &'static str,
      id:      u64,
      method:  &'static str,
      params:  Box<RawValue>,
    }

    let content = serde_json::to_string(&Request {
      jsonrpc: "2.0",
      id:      request.id,
      method:  request.method,
      params:  request.params,
    })
    .expect("serialize request");

    write!(self.writer, "Content-Length: {}\r\n\r\n{}", content.len(), content)?;
    Ok(())
  }

  fn response(&mut self, id: u64, result: &RawValue) -> io::Result<()> {
    #[derive(serde::Serialize)]
    struct Response<'a> {
      jsonrpc: &'static str,
      id:      u64,
      result:  &'a RawValue,
    }

    let content =
      serde_json::to_string(&Response { jsonrpc: "2.0", id, result }).expect("serialize response");

    write!(self.writer, "Content-Length: {}\r\n\r\n{}", content.len(), content)?;
    Ok(())
  }

  fn notify(&mut self, req: Notification) -> io::Result<()> {
    #[derive(serde::Serialize)]
    struct Notification {
      jsonrpc: &'static str,
      method:  &'static str,
      params:  Box<RawValue>,
    }

    let content = serde_json::to_string(&Notification {
      jsonrpc: "2.0",
      method:  req.method,
      params:  req.params,
    })
    .expect("serialize notification");

    write!(self.writer, "Content-Length: {}\r\n\r\n{}", content.len(), content)?;
    Ok(())
  }
}

#[allow(dead_code)]
pub enum Message {
  Request { id: u64, method: String, params: Option<Box<RawValue>> },
  Response { id: u64, result: Box<RawValue> },
  Error { id: u64, error: Box<RawValue> },
  Notification { method: String, params: Option<Box<RawValue>> },
}

impl Reader {
  fn new(stdout: ChildStdout) -> Reader { Reader { reader: stdout, read: VecDeque::new() } }

  fn recv(&mut self) -> io::Result<Option<Message>> {
    if let Some(msg) = self.decode()? {
      return Ok(Some(msg));
    }

    loop {
      let mut buf = [0u8; 1024];
      match self.reader.read(&mut buf) {
        Ok(0) => {
          return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "LSP server closed connection"));
        }
        Ok(n) => self.read.extend(&buf[..n]),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
        Err(e) => return Err(e),
      }
    }

    self.decode()
  }

  fn decode(&mut self) -> io::Result<Option<Message>> {
    let mut iter = self.read.iter();
    let mut prev = 0;
    let mut len = None;
    loop {
      let Some(pos) = iter.position(|c| *c == b'\n') else {
        return Ok(None);
      };
      let terminator = pos + 1;

      let header = self.read.range(prev..prev + terminator).copied().collect::<Vec<u8>>();
      let header = String::from_utf8_lossy(&header);
      let header = header.trim();
      prev += terminator;

      if header == "" {
        break;
      }

      let Some((key, value)) = header.split_once(':') else { continue };
      let key = key.trim();
      let value = value.trim();

      match key {
        "Content-Length" => {
          len =
            Some(value.parse::<u32>().map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?);
        }

        _ => {}
      }
    }

    let Some(len) = len else { return Ok(None) };

    if self.read.len() < prev + len as usize {
      return Ok(None);
    }

    self.read.drain(..prev);
    let msg = self.read.drain(..len as usize).collect::<Vec<u8>>();

    serde_json::from_slice::<Message>(&msg)
      .map(Some)
      .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
  }
}

#[derive(Debug)]
enum Field {
  Jsonrpc,
  Id,
  Result,
  Error,
  Method,
  Params,
  Other,
}

impl<'de> de::Deserialize<'de> for Field {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: de::Deserializer<'de>,
  {
    struct FieldVisitor;

    impl<'de> de::Visitor<'de> for FieldVisitor {
      type Value = Field;

      fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON-RPC field name")
      }

      fn visit_str<E>(self, v: &str) -> Result<Field, E>
      where
        E: de::Error,
      {
        Ok(match v {
          "jsonrpc" => Field::Jsonrpc,
          "id" => Field::Id,
          "result" => Field::Result,
          "error" => Field::Error,
          "method" => Field::Method,
          "params" => Field::Params,
          _ => Field::Other,
        })
      }

      fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Field, E>
      where
        E: de::Error,
      {
        self.visit_str(v)
      }
    }

    deserializer.deserialize_identifier(FieldVisitor)
  }
}

impl<'de> de::Deserialize<'de> for Message {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: de::Deserializer<'de>,
  {
    struct MsgVisitor;

    impl<'de> de::Visitor<'de> for MsgVisitor {
      type Value = Message;

      fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON-RPC message object")
      }

      fn visit_map<A>(self, mut map: A) -> Result<Message, A::Error>
      where
        A: de::MapAccess<'de>,
      {
        let mut jsonrpc: Option<&str> = None;

        let mut id: Option<u64> = None;
        let mut result: Option<Box<RawValue>> = None;
        let mut error: Option<Box<RawValue>> = None;

        let mut method: Option<String> = None;
        let mut params: Option<Box<RawValue>> = None;

        while let Some(key) = map.next_key::<Field>()? {
          macro_rules! fields {
            ($($field:ident: $var:ident,)*) => {
              match key {
                $(
                  Field::$field => {
                    if $var.is_some() {
                      return Err(de::Error::duplicate_field(stringify!($var)));
                    }

                    $var = Some(map.next_value()?);
                  }
                )*

                Field::Other => {
                  map.next_value::<de::IgnoredAny>()?;
                }
              }
            }
          }

          fields!(
            Jsonrpc: jsonrpc,
            Id: id,
            Result: result,
            Error: error,
            Method: method,
            Params: params,
          );
        }

        if let Some(v) = jsonrpc {
          if v != "2.0" {
            return Err(de::Error::custom("unsupported jsonrpc version"));
          }
        }

        match (method, id, params, result, error) {
          (None, Some(id), None, Some(result), None) => Ok(Message::Response { id, result }),
          (None, Some(id), None, None, Some(error)) => Ok(Message::Error { id, error }),
          (Some(method), Some(id), params, None, None) => {
            Ok(Message::Request { id, method, params })
          }
          (Some(method), None, params, None, None) => Ok(Message::Notification { method, params }),

          _ => Err(de::Error::custom("invalid or ambiguous JSON-RPC message")),
        }
      }
    }

    deserializer.deserialize_map(MsgVisitor)
  }
}

#[cfg(test)]
mod tests {

  use super::*;

  #[test]
  fn spawn_client() {
    let (mut client, _) = LspClient::spawn("rust-analyzer", Arc::new(Mutex::new(Box::new(|| {}))));

    let path = std::path::Path::new("./src/lib.rs").canonicalize().unwrap();
    let uri = types::Uri::from_file_path(&path);

    client.notify::<lsp::notification::TextDocumentDidOpen>(lsp::DidOpenTextDocumentParams {
      text_document: lsp::TextDocumentItem {
        uri:         uri.clone(),
        text:        std::fs::read_to_string(&path).unwrap(),
        version:     1,
        language_id: "rust".into(),
      },
    });

    let task = client.request::<lsp::request::TextDocumentCompletion>(lsp::CompletionParams {
      work_done_progress_params:     Default::default(),
      text_document_position_params: lsp::TextDocumentPositionParams {
        text_document: lsp::TextDocumentIdentifier { uri },
        position:      lsp::Position { line: 0, character: 0 },
      },
      context:                       None,
      partial_result_params:         Default::default(),
    });

    loop {
      let res = task.completed();
      match res {
        Some(res) => {
          println!("res: {:#?}", res);
          break;
        }
        None => std::thread::sleep(std::time::Duration::from_millis(100)),
      }
    }

    client.shutdown();
  }
}
