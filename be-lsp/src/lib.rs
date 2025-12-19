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

    client
  }

  fn send<T: lsp_types::request::Request>(&mut self, req: T::Params) {
    #[derive(serde::Serialize)]
    struct Message<P> {
      jsonrpc: &'static str,
      id:      u64,
      method:  &'static str,
      params:  P,
    }

    let content = serde_json::to_string(&Message {
      jsonrpc: "2.0",
      id:      1,
      method:  T::METHOD,
      params:  req,
    })
    .unwrap();

    let message = format!("Content-Length: {}\r\n\r\n{}", content.len(), content);
    self.tx.writer.write_all(message.as_bytes()).unwrap();
  }
}

impl Sender {
  fn new(stdin: ChildStdin) -> Sender { Sender { messages: Vec::new(), writer: stdin } }
}

impl Receiver {
  fn new(stdout: ChildStdout) -> Receiver { Receiver { reader: stdout, read: VecDeque::new() } }

  fn recv(&mut self) -> Option<String> {
    if let Some(msg) = self.decode() {
      return Some(msg);
    }

    let mut buf = [0u8; 1024];
    let read = self.reader.read(&mut buf).unwrap();
    self.read.extend(&buf[..read]);

    self.decode()
  }

  fn decode(&mut self) -> Option<String> {
    let terminator = self.read.iter().position(|c| *c == b'\n')?;

    let msg = self.read.drain(..=terminator).collect::<Vec<u8>>();

    Some(String::from_utf8_lossy(&msg).to_string().trim().to_string())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn spawn_client() {
    let mut client = LspClient::spawn("rust-analyzer");

    dbg!(client.rx.recv());
    panic!();
  }
}
