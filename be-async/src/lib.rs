use std::io;

use polling::AsRawSource;

pub fn set_nonblocking(source: impl AsRawSource) -> io::Result<()> {
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
