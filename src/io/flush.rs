use std::io as std_io;

use futures::{Async, Future, Poll};
use tokio::io::AsyncWrite;

use super::Io;

impl Io {
    /// return a futures resolving back to this instance once all output data is flushed
    pub fn flush(self) -> Flushing {
        Flushing::new(self)
    }

    /// writes `cmd` and then `"\r\n"` to `buffer.input` and then calls `flush`
    pub fn flush_line_from_parts(mut self, line: &[&str]) -> Flushing {
        self.write_line_from_parts(line);
        self.flush()
    }

    /// writes data from the output buffer to the socket and polls flush
    ///
    /// This first poll the writing of data from output to socket until
    /// output is empty, then it will start polling flush on the socket.
    pub fn poll_flush(&mut self) -> Poll<(), std_io::Error> {
        let output = &mut self.buffer.output;
        let socket = &mut self.socket;
        while !output.is_empty() {
            let n = try_ready!(socket.poll_write(output));

            // as long as output is not empty a it should never write 0 bytes
            assert!(n > 0);

            // remove the bytes written from the buffer
            output.advance(n);
        }

        try_ready!(socket.poll_flush());

        Ok(Async::Ready(()))
    }
}

pub struct Flushing {
    inner: Option<Io>,
}

impl Flushing {
    pub(crate) fn new(inner: Io) -> Self {
        #[cfg(feature = "log")]
        {
            use log_facade::*; // This is needed due to something which is probably a rustc bug.
            if log_enabled!(Level::Trace) {
                let out = &inner.buffer.output[..];
                let out = String::from_utf8_lossy(out);
                for line in out.lines() {
                    if line.starts_with("AUTH") {
                        let additional_chars_for_auth_subcommand =
                            line[5..].bytes().position(|ch| ch == b' ').unwrap_or(0);
                        let end = 5 + additional_chars_for_auth_subcommand;
                        log_facade::trace!("C: {:?} <redacted>", &line[..end]);
                    } else {
                        log_facade::trace!("C: {:?}", line);
                    }
                }
            }
        }

        Flushing { inner: Some(inner) }
    }
}

impl Future for Flushing {
    type Item = Io;
    type Error = std_io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        try_ready!({
            let io = self.inner.as_mut().expect("poll after completion");
            io.poll_flush()
        });

        let io = self.inner.take().unwrap();
        Ok(Async::Ready(io))
    }
}
