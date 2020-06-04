use std::{io as std_io, mem};

use bytes::BufMut;
use futures::{Async, Future, Poll};
use tokio::io::AsyncRead;

use super::{Io, SmtpResult, INPUT_BUFFER_INC_SIZE};
use crate::{error::check_response, response::parser};

impl Io {
    /// parse a "normal" smtp response
    ///
    /// # Panics
    ///
    /// Panics if the write buffer is not empty
    pub fn parse_response(self) -> Parsing {
        if !self.buffer.output.is_empty() {
            panic!("parsing input before writing all output")
        }
        Parsing::new(self)
    }

    /// read data from the socket to buffer.input until it would block or the socket closed
    ///
    /// The input buffer is increased in increments of 256 bytes (`INPUT_BUFFER_INC_SIZE`)
    pub fn read_from_socket(&mut self) -> Result<ReadState, std_io::Error> {
        let input = &mut self.buffer.input;
        let socket = &mut self.socket;

        //TODO limit the buffer size (configurable) to limit smtp response line size
        loop {
            if input.remaining_mut() == 0 {
                input.reserve(INPUT_BUFFER_INC_SIZE);
            }

            match socket.read_buf(input) {
                Ok(Async::NotReady) => return Ok(ReadState::NotReady),
                Ok(Async::Ready(0)) => return Ok(ReadState::SocketClosed),
                Ok(Async::Ready(_)) => (),
                Err(err) => return Err(err),
            }
        }
    }

    /// # Implementation Limitations
    ///
    /// Be aware that try_read_line does only work on continuous buffers.
    /// I.e. it would fail if `self.in_buffer()` is a `Chain`
    pub fn try_pop_line<F, R, E>(&mut self, parse_line_fn: F) -> Result<Option<R>, E>
    where
        F: FnOnce(&[u8]) -> Result<R, E>,
    {
        let input = self.in_buffer();

        let eol = (&*input).windows(2).position(|pair| pair == b"\r\n");

        if let Some(eol) = eol {
            let line = &input[..eol];
            let parsed = parse_line_fn(line)?;
            input.advance(eol + 2);
            Ok(Some(parsed))
        } else {
            Ok(None)
        }
    }
}

/// Used to hint if a socket was closed
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ReadState {
    /// the socket was closed when reading from it
    SocketClosed,
    /// the socket is not ready
    NotReady,
    // Buffer full is in between read and not ready, and super annoying to
    // handle (e.g. the edge case where the buffer is full and does not contain
    // at last one complete line, and the part that you can not "just" return
    // Ready as well it's just partially ready and you can also not return
    // NotReady as there is no Wakup registered)
    // For now this will not be handle, maybe a max sized buffer + error if more requested
    // is enough, I mean it's a smtp _Client_ it mainly gets back status messages etc. just
    // some comands like list all users could actually fill the buffer (if decent sized),
    // but then this commands do exists...
    //BufferFull,
}

impl ReadState {
    pub fn is_socket_closed(self) -> bool {
        self == ReadState::SocketClosed
    }
}

/// future returned by `Connection.parse_result`
pub struct Parsing {
    inner: Option<Io>,
    lines: Vec<parser::ResponseLine>,
}

impl Parsing {
    pub(crate) fn new(inner: Io) -> Self {
        Parsing {
            inner: Some(inner),
            lines: Vec::new(),
        }
    }

    fn io_mut(&mut self) -> &mut Io {
        self.inner.as_mut().expect("[BUG] poll after completion")
    }

    fn read_result(&mut self) -> Result<Option<(Io, SmtpResult)>, parser::ParseError> {
        loop {
            let opt_line = self
                .io_mut()
                .try_pop_line(|line| parser::parse_line(line))?;

            if let Some(line) = opt_line {
                let last = line.last_line;
                self.lines.push(line);

                if !last {
                    continue;
                }

                let lines = mem::replace(&mut self.lines, Vec::new());
                let response = parser::response_from_parsed_lines(lines.into_iter())?;

                let io = self.inner.take().expect("[BUG] poll after completion");
                //FIXME[buf_management]: maybe normalize output bufer to have at most cap of 1024
                return Ok(Some((io, check_response(response))));
            } else {
                return Ok(None);
            }
        }
    }
}

impl Future for Parsing {
    type Item = (Io, SmtpResult);
    type Error = std_io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        //1. parse more data
        let state = self.io_mut().read_from_socket()?;

        //2. see if we have a full response now
        match self.read_result() {
            Ok(Some(result)) => return Ok(Async::Ready(result)),
            Ok(None) => (),
            Err(err) => return Err(std_io::Error::new(std_io::ErrorKind::InvalidData, err)),
        }

        //3. if not see if the socked was closed
        match state {
            ReadState::NotReady => return Ok(Async::NotReady),
            ReadState::SocketClosed => {
                return Err(std_io::Error::new(
                    std_io::ErrorKind::ConnectionAborted,
                    "socked closed before getting full smtp response",
                ));
            }
        }
    }
}
