use be_task::Task;
use polling::{AsRawSource, Events, Poller};
use serde::de;
use serde_json::value::RawValue;
use std::{
  collections::{HashMap, VecDeque},
  fmt,
  io::{self, Read, Write},
  process::{Child, ChildStdin, ChildStdout, Stdio},
  sync::Arc,
};

mod init;

pub extern crate lsp_types as types;

pub struct LspClient {
  #[allow(dead_code)]
  child: Child,

  poller: Arc<Poller>,
  tx:     crossbeam_channel::Sender<LspRequest>,
  rx:     crossbeam_channel::Receiver<Message>,
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

struct LspWorker {
  rx: crossbeam_channel::Receiver<LspRequest>,
  tx: crossbeam_channel::Sender<Message>,

  poller: Arc<Poller>,
  writer: Writer,
  reader: Reader,

  pending: HashMap<u64, Completer>,
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
  pub fn spawn(cmd: &str) -> (LspClient, lsp_types::ServerCapabilities) {
    let mut child =
      std::process::Command::new(cmd).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();

    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    let (send_tx, send_rx) = crossbeam_channel::unbounded();
    let (recv_tx, recv_rx) = crossbeam_channel::unbounded();

    let worker = LspWorker {
      rx:      send_rx,
      tx:      recv_tx,
      poller:  Arc::new(Poller::new().unwrap()),
      writer:  Writer::new(stdin),
      reader:  Reader::new(stdout),
      pending: HashMap::new(),
    };
    let poller = worker.poller.clone();
    std::thread::spawn(move || worker.run());

    let mut client = LspClient { child, poller, tx: send_tx, rx: recv_rx };

    let init = lsp_types::InitializeParams {
      process_id: Some(std::process::id()),
      capabilities: init::client_capabilities(),
      ..Default::default()
    };

    let task = client.send::<lsp_types::request::Initialize>(init);

    let result = loop {
      match task.completed() {
        Some(msg) => break msg,
        None => {
          std::thread::sleep(std::time::Duration::from_millis(1));
        }
      }
    };

    client.notify::<lsp_types::notification::Initialized>(lsp_types::InitializedParams {});

    (client, result.capabilities)
  }

  pub fn send<T: lsp_types::request::Request>(&mut self, req: T::Params) -> Task<T::Result> {
    let task = Task::new();

    let completer = task.completer();
    self
      .tx
      .send(LspRequest::Request(
        Request {
          id:     1,
          method: T::METHOD,
          params: RawValue::from_string(serde_json::to_string(&req).unwrap()).unwrap(),
        },
        Box::new(move |value| {
          let result = serde_json::from_str(&value.get()).unwrap();
          match completer.complete(result) {
            Ok(()) => {}
            Err(_) => {} // already completed. this is probably an error
          }
        }),
      ))
      .unwrap();
    self.poller.notify().unwrap();

    task
  }

  pub fn notify<T: lsp_types::notification::Notification>(&mut self, req: T::Params) {
    self
      .tx
      .send(LspRequest::Notification(Notification {
        method: T::METHOD,
        params: RawValue::from_string(serde_json::to_string(&req).unwrap()).unwrap(),
      }))
      .unwrap();
    self.poller.notify().unwrap();
  }

  pub fn shutdown(&mut self) { self.notify::<lsp_types::notification::Exit>(()); }
}

fn set_nonblocking(source: impl AsRawSource) -> io::Result<()> {
  unsafe {
    let flags = libc::fcntl(source.raw(), libc::F_GETFL);
    if flags < 0 {
      return Err(io::Error::last_os_error());
    }

    if flags & libc::O_NONBLOCK != 0 {
      return Ok(());
    }

    if libc::fcntl(source.raw(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
      return Err(io::Error::last_os_error());
    }

    Ok(())
  }
}

impl LspWorker {
  pub fn run(mut self) {
    const READ: usize = 0;
    const WRITE: usize = 1;

    set_nonblocking(&self.reader.reader).unwrap();
    set_nonblocking(&self.writer.writer).unwrap();

    let poller = Poller::new().unwrap();
    // SAFETY: These are removed down below.
    unsafe {
      poller.add(&self.reader.reader, polling::Event::readable(READ)).unwrap();
      poller.add(&self.writer.writer, polling::Event::writable(WRITE)).unwrap();
    }

    'outer: loop {
      let mut events = Events::new();

      poller.wait(&mut events, None).unwrap();
      for ev in events.iter() {
        match ev.key {
          READ => {
            while let Some(msg) = self.reader.recv() {
              match msg {
                Message::Request { method, .. } => println!("request: {}", method),
                Message::Notification { method, .. } => println!("notification: {}", method),
                Message::Response { id, result, .. } => {
                  if let Some(completer) = self.pending.remove(&id) {
                    completer(&result);
                  }
                }
                Message::Error { id, .. } => {
                  if let Some(_) = self.pending.remove(&id) {
                    println!("error: {id}");
                  }
                }
              }
            }
          }
          WRITE => {
            println!("writable");
          }

          _ => panic!("unexpected event"),
        }
      }

      loop {
        match self.rx.try_recv() {
          Ok(LspRequest::Request(req, completer)) => {
            self.pending.insert(req.id, completer);
            self.writer.request(req);
          }
          Ok(LspRequest::Notification(req)) => self.writer.notify(req),
          Err(crossbeam_channel::TryRecvError::Empty) => break,
          Err(crossbeam_channel::TryRecvError::Disconnected) => break 'outer,
        }
      }
    }

    poller.delete(&self.reader.reader).unwrap();
    poller.delete(&self.writer.writer).unwrap();
  }
}

impl Writer {
  fn new(stdin: ChildStdin) -> Writer { Writer { writer: stdin } }

  fn request(&mut self, request: Request) {
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
    .unwrap();

    write!(self.writer, "Content-Length: {}\r\n\r\n{}", content.len(), content).unwrap();
  }

  fn notify(&mut self, req: Notification) {
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
    .unwrap();

    write!(self.writer, "Content-Length: {}\r\n\r\n{}", content.len(), content).unwrap();
  }
}

pub enum Message {
  Request { id: u64, method: String, params: Option<Box<RawValue>> },
  Response { id: u64, result: Box<RawValue> },
  Error { id: u64, error: Box<RawValue> },
  Notification { method: String, params: Option<Box<RawValue>> },
}

impl Reader {
  fn new(stdout: ChildStdout) -> Reader { Reader { reader: stdout, read: VecDeque::new() } }

  fn recv(&mut self) -> Option<Message> {
    if let Some(msg) = self.decode() {
      return Some(msg);
    }

    loop {
      let mut buf = [0u8; 1024];
      match self.reader.read(&mut buf) {
        Ok(0) => panic!("EOF"),
        Ok(n) => self.read.extend(&buf[..n]),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
        Err(e) => panic!("{}", e),
      }
    }

    self.decode()
  }

  fn decode(&mut self) -> Option<Message> {
    let mut iter = self.read.iter();
    let mut prev = 0;
    let mut len = None;
    loop {
      let terminator = iter.position(|c| *c == b'\n')? + 1;

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
          len = Some(value.parse::<u32>().unwrap());
        }

        _ => {}
      }
    }

    let Some(len) = len else { return None };

    if self.read.len() < prev + len as usize {
      return None;
    }

    self.read.drain(..prev);
    let msg = self.read.drain(..len as usize).collect::<Vec<u8>>();

    Some(serde_json::from_slice::<Message>(&msg).unwrap())
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
        let mut params: Option<Option<Box<RawValue>>> = None;

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
          (Some(method), Some(id), Some(params), None, None) => {
            Ok(Message::Request { id, method, params })
          }
          (Some(method), None, Some(params), None, None) => {
            Ok(Message::Notification { method, params })
          }

          _ => Err(de::Error::custom("invalid or ambiguous JSON-RPC message")),
        }
      }
    }

    deserializer.deserialize_map(MsgVisitor)
  }
}

#[cfg(test)]
mod tests {
  use std::str::FromStr;

  use types::Uri;

  use super::*;

  #[test]
  fn spawn_client() {
    let (mut client, _) = LspClient::spawn("rust-analyzer");

    let path = std::path::Path::new("./src/lib.rs").canonicalize().unwrap();
    let uri = Uri::from_str(&format!("file://{}", path.to_str().unwrap())).unwrap();

    let task = client.send::<lsp_types::request::GotoDefinition>(lsp_types::GotoDefinitionParams {
      work_done_progress_params:     Default::default(),
      text_document_position_params: lsp_types::TextDocumentPositionParams {
        text_document: lsp_types::TextDocumentIdentifier { uri },
        position:      lsp_types::Position { line: 0, character: 0 },
      },
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
  }
}
