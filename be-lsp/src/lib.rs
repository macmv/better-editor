use serde::de;
use serde_json::value::RawValue;
use std::{
  collections::VecDeque,
  fmt,
  io::{Read, Write},
  process::{Child, ChildStdin, ChildStdout, Stdio},
};

mod init;

pub extern crate lsp_types as lsp;

pub struct LspClient {
  #[allow(dead_code)]
  child: Child,

  tx: Sender,
  rx: Receiver,
}

struct Sender {
  writer: ChildStdin,
}

struct Receiver {
  reader: ChildStdout,
  read:   VecDeque<u8>,
}

impl LspClient {
  pub fn spawn(cmd: &str) -> (LspClient, lsp_types::ServerCapabilities) {
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

    let result: lsp_types::InitializeResult = serde_json::from_str(&result.get()).unwrap();

    client.notify::<lsp_types::notification::Initialized>(lsp_types::InitializedParams {});

    (client, result.capabilities)
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
}

impl Sender {
  fn new(stdin: ChildStdin) -> Sender { Sender { writer: stdin } }
}

pub enum Message {
  Request { id: u64, method: String, params: Option<Box<RawValue>> },
  Response { id: u64, result: Box<RawValue> },
  Error { id: u64, error: Box<RawValue> },
  Notification { method: String, params: Option<Box<RawValue>> },
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
  use super::*;

  #[test]
  fn spawn_client() {
    let (mut client, _) = LspClient::spawn("rust-analyzer");

    while client.rx.recv().is_none() {}
  }
}
