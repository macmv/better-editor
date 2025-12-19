use std::{
  collections::VecDeque,
  io::{Read, Write},
  process::{Child, ChildStdin, ChildStdout, Stdio},
};

mod init;

pub struct LspClient {
  child: Child,

  tx: Sender,
  rx: Receiver,
}

struct Sender {
  messages: Vec<String>,
  writer:   ChildStdin,
}

struct Receiver {
  reader: ChildStdout,
  read:   VecDeque<u8>,
}

impl LspClient {
  pub fn spawn(cmd: &str) -> LspClient {
    let mut child =
      std::process::Command::new(cmd).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();

    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    let mut client = LspClient { child, tx: Sender::new(stdin), rx: Receiver::new(stdout) };

    let init = lsp_types::InitializeParams {
      process_id: Some(std::process::id()),
      capabilities: init::client_capabilities(),
      ..Default::default()
    };

    client.send::<lsp_types::request::Initialize>(init);

    let msg = loop {
      match client.rx.recv() {
        Some(msg) => break msg,
        None => {}
      }
    };

    let result = match msg {
      Message::Response { result, .. } => result,
      _ => panic!(),
    };

    let _result: lsp_types::InitializeResult = serde_json::from_str(&result.get()).unwrap();

    client.notify::<lsp_types::notification::Initialized>(lsp_types::InitializedParams {});

    client
  }

  fn send<T: lsp_types::request::Request>(&mut self, req: T::Params) {
    #[derive(serde::Serialize)]
    struct Request<P> {
      jsonrpc: &'static str,
      id:      u64,
      method:  &'static str,
      params:  P,
    }

    let content = serde_json::to_string(&Request {
      jsonrpc: "2.0",
      id:      1,
      method:  T::METHOD,
      params:  req,
    })
    .unwrap();

    write!(self.tx.writer, "Content-Length: {}\r\n\r\n{}", content.len(), content).unwrap();
  }

  fn notify<T: lsp_types::notification::Notification>(&mut self, req: T::Params) {
    #[derive(serde::Serialize)]
    struct Notification<P> {
      jsonrpc: &'static str,
      method:  &'static str,
      params:  P,
    }

    let content =
      serde_json::to_string(&Notification { jsonrpc: "2.0", method: T::METHOD, params: req })
        .unwrap();

    write!(self.tx.writer, "Content-Length: {}\r\n\r\n{}", content.len(), content).unwrap();
  }

  fn recv(&self) {}
}

impl Sender {
  fn new(stdin: ChildStdin) -> Sender { Sender { messages: Vec::new(), writer: stdin } }
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Message {
  Request { id: u64, method: String, params: Box<serde_json::value::RawValue> },
  Response { id: u64, result: Box<serde_json::value::RawValue> },
  Notification { method: String, params: Box<serde_json::value::RawValue> },
}

impl Receiver {
  fn new(stdout: ChildStdout) -> Receiver { Receiver { reader: stdout, read: VecDeque::new() } }

  fn recv(&mut self) -> Option<Message> {
    if let Some(msg) = self.decode() {
      return Some(msg);
    }

    let mut buf = [0u8; 1024];
    let read = self.reader.read(&mut buf).unwrap();
    self.read.extend(&buf[..read]);

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

    println!("msg: {}", String::from_utf8_lossy(&msg));

    Some(serde_json::from_slice::<Message>(&msg).unwrap())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn spawn_client() {
    let mut client = LspClient::spawn("rust-analyzer");

    while client.rx.recv().is_none() {}
  }
}
