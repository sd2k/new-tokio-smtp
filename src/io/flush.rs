use std::io as std_io;

use futures::{Poll, Future, Async};
use tokio::io::AsyncWrite;

use super::Io;


impl Io {
    pub fn flush(self) -> Flushing {
        Flushing::new(self)
    }

    /// writes `cmd` and then `"\r\n"` to `buffer.input` and then calls `flush`
    pub fn flush_line_from_parts(mut self, line: &[&str]) -> Flushing {
        self.write_line_from_parts(line);
        self.flush()
    }

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
    inner: Option<Io>
}

impl Flushing {
    pub(crate) fn new(inner: Io) -> Self {
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

