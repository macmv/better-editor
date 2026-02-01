use std::error::Error;

pub struct Status {
  pub message: String,
  pub success: bool,
}

impl Status {
  pub fn for_success(message: impl Into<String>) -> Self {
    Status { message: message.into(), success: true }
  }
  pub fn for_error(e: impl Error) -> Self { Status { message: e.to_string(), success: false } }
}
