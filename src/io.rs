use std::io as std_io;
use std::mem;

use futures::{Poll, Future, Async};
use futures::stream::Stream;
use bytes::BytesMut;
use bytes::buf::{Buf, BufMut};
use tokio::net::TcpStream;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tls::TlsStream;

use ::response::{Response, parser};



pub type SmtpResult = Result<Response, Response>;

/// Used to hint if a socket was closed
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ReadState {
    SocketClosed,
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

#[derive(Debug)]
pub struct Io {
    socket: Socket,
    buffer: Buffers,
}

impl Io {

    pub fn is_secure(&self) -> bool {
        self.socket.is_secure()
    }

    pub fn out_buffer(&mut self) -> &mut BytesMut {
        &mut self.buffer.output
    }

    pub fn in_buffer(&mut self) -> &mut BytesMut {
        &mut self.buffer.input
    }

    pub fn destruct(self) -> (Socket, Buffers) {
        let Io { socket, buffer } = self;
        (socket, buffer)
    }

    /// writes <cmd> and then "\r\n" and then calls flush
    pub fn flush_cmd(mut self, cmd: &str) -> Flushing {
        self.in_buffer().put(cmd);
        self.in_buffer().put("\r\n");
        self.flush()
    }

    pub fn flush(self) -> Flushing {
        Flushing::new(self)
    }

    pub fn write_dot_stashed<S>(self, source: S) -> DotStashedWrite<S>
        where S: Stream<Error=std_io::Error>, S::Item: Buf
    {
        DotStashedWrite::new(self, source)
    }

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

    fn poll_flush(&mut self) -> Poll<(), std_io::Error> {
        let output = &mut self.buffer.output;
        let socket = &mut self.socket;
        while !output.is_empty() {
            let n = try_ready!(socket.poll_write(output));

            // as long as output is not empty a it should never write 0 bytes
            assert!(n > 0);

            // remove the bytes written from the buffer
            output.advance(n);
        }

        Ok(Async::Ready(()))
    }

    fn read_from_socket(&mut self) -> Result<ReadState, std_io::Error> {
        let input = &mut self.buffer.input;
        let socket = &mut self.socket;

        loop {
            //TODO limit the buffer size (configurable) to limit smtp response line size
            // reverse more buffer (this is currently _not_ limited,
            // through limiting it needs special handling wrt. to
            // notifying once the buffer is less full)
            //
            // if buffer size is not limited in a if-full-error way the containing loop
            // has to be replicated at the outside including a consumer of the buffer
            input.reserve(1024);

            // read as many bytes as possible
            // if not ready then return
            match socket.read_buf(input) {
                Ok(Async::NotReady) => return Ok(ReadState::NotReady),
                Ok(Async::Ready(0)) => return Ok(ReadState::SocketClosed),
                Ok(Async::Ready(_)) => (),
                Err(err) => return Err(err)
            }
        }
    }

    fn try_read_line<F, R, E>(&mut self, parse_line_fn: F)
        -> Result<Option<R>, E>
        where F: FnOnce(&[u8]) -> Result<R, E>
    {
        let input = self.in_buffer();

        let eol = (&*input)
            .windows(2)
            .enumerate()
            .find(|&(_idx, pair)| pair == b"\r\n")
            .map(|(idx, _)| idx);

        if let Some(eol) = eol {
            // passes in without line ending
            let parsed = parse_line_fn(&input[..eol])?;
            // advance through line ending
            input.advance(eol + 2);
            // the start of the buffer was moved to eol + 2
            // so now return 0 as new scan offset
            Ok(Some(parsed))
        } else {
            Ok(None)
        }
    }


}

impl From<(Socket, Buffers)> for Io {
    fn from((socket, buffer): (Socket, Buffers)) -> Self {
        Io { socket, buffer }
    }
}

#[derive(Debug)]
pub struct Buffers {
    pub input: BytesMut,
    pub output: BytesMut,
}

impl Buffers {

    pub fn new() -> Self {
        Buffers {
            input: BytesMut::new(),
            output: BytesMut::new()
        }
    }
}

//pub trait MockStream: Debug + AsyncRead + AsyncWrite + 'static {
//    fn is_secure(&self) -> bool {
//        false
//    }
//}

#[derive(Debug)]
pub enum Socket {
    Secure(TlsStream<TcpStream>),
    Insecure(TcpStream),
    //Mock(Box<MockStream>)
}

impl Socket {

    pub fn is_secure(&self) -> bool {
        match *self {
            Socket::Secure(_) => true,
            Socket::Insecure(_) => false,
            //Socket::Mock(ref mock) => mock.is_secure()
        }
    }
}

macro_rules! socket_mux {
    ($self:ident, |$socket:ident| $block:block) => ({
        match *$self {
            Socket::Secure(ref mut $socket) => $block,
            Socket::Insecure(ref mut $socket) => $block,
            //Socket::Mock(ref mut $socket) => $block
        }
    });
}

impl std_io::Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std_io::Error> {
        socket_mux! {self, |socket| {
            socket.read(buf)
        }}
    }
}

impl std_io::Write for Socket {
    fn write(&mut self, buf: &[u8]) -> Result<usize, std_io::Error> {
        socket_mux! {self, |socket| {
            socket.write(buf)
        }}
    }

    fn flush(&mut self) -> Result<(), std_io::Error> {
        socket_mux! {self, |socket| {
            socket.flush()
        }}
    }
}

impl AsyncRead for Socket {
//    #[inline]
//    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
//        socket_mux! {self, |socket| {
//            socket.prepare_uninitialized_buffer(buf)
//        }}
//    }

    #[inline]
    fn poll_read(&mut self, buf: &mut [u8]) -> Poll<usize, std_io::Error> {
        socket_mux! {self, |socket| {
            socket.poll_read(buf)
        }}
    }

    #[inline]
    fn read_buf<B: BufMut>(&mut self, buf: &mut B) -> Poll<usize, std_io::Error>
        where Self: Sized,
    {
        socket_mux! {self, |socket| {
            socket.read_buf(buf)
        }}
    }
}

impl AsyncWrite for Socket {
    fn poll_write(&mut self, buf: &[u8]) -> Poll<usize, std_io::Error> {
        socket_mux! {self, |socket| {
            AsyncWrite::poll_write(socket, buf)
        }}
    }

    fn poll_flush(&mut self) -> Poll<(), std_io::Error> {
        socket_mux! {self, |socket| {
            AsyncWrite::poll_flush(socket)
        }}
    }

    fn shutdown(&mut self) -> Poll<(), std_io::Error> {
        socket_mux! {self, |socket| {
            AsyncWrite::shutdown(socket)
        }}
    }

    fn write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, std_io::Error>
        where Self: Sized,
    {
        socket_mux! {self, |socket| {
            AsyncWrite::write_buf(socket, buf)
        }}
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

pub struct Parsing {
    inner: Option<Io>,
    lines: Vec<parser::ResponseLine>
}

impl Parsing {
    pub(crate) fn new(inner: Io) -> Self {
        Parsing {
            inner: Some(inner),
            lines: Vec::new()
        }
    }

    fn io_mut(&mut self) -> &mut Io {
        self.inner.as_mut().expect("[BUG] poll after completion")
    }

    fn read_result(&mut self) -> Result<Option<(Io, SmtpResult)>, parser::ParseError> {
        loop {
            let opt_line = self
                .io_mut()
                .try_read_line(|line| parser::parse_line(line) )?;

            if let Some(line) = opt_line {
                let last = line.last_line;
                self.lines.push(line);

                if !last {
                    continue;
                }

                let lines = mem::replace(&mut self.lines, Vec::new());
                let response = parser::response_from_parsed_lines(lines.into_iter())?;

                let io = self.inner.take().expect("[BUG] poll after completion");
                return Ok(Some((io, response.into_result())));

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
            Err(err) => return Err(std_io::Error::new(
                std_io::ErrorKind::InvalidData, err
            ))
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

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
enum CrLf {
    None,
    HitCr,
    HitLf
}

pub struct DotStashedWrite<S>
    where S: Stream, S::Item: Buf
{
    io: Option<Io>,
    source: S,
    stash_state: CrLf,
    write_eom_seq: bool
}

impl<S> DotStashedWrite<S>
    where S: Stream<Error=std_io::Error>, S::Item: Buf
{
    fn new(io: Io, source: S) -> Self {
        DotStashedWrite {
            source,
            io: Some(io),
            stash_state: CrLf::None,
            write_eom_seq: false
        }
    }

    fn io_mut(&mut self) -> &mut Io {
        self.io.as_mut().expect("poll after completion")
    }

    fn poll_source(&mut self) -> Poll<Option<S::Item>, std_io::Error> {
        let next = try_ready!(self.source.poll());

        if next.is_none() {
            self.write_eom_seq = true;
            let stash_state = self.stash_state;

            let out = self.io_mut().out_buffer();
            if stash_state != CrLf::HitLf {
                out.put("\r\n");
            }
            out.put(".\r\n");
        }

        Ok(Async::Ready(next))
    }

    fn write_dot_stashed_output(&mut self, unstashed: S::Item) {
        //write from source into buffer
        let mut state = self.stash_state;
        {
            let out = self.io_mut().out_buffer();
            for bch in unstashed.iter() {
                let (stash, new_state) = match (bch, state) {
                    (b'\r', CrLf::None) => (false, CrLf::HitCr),
                    (b'\n', CrLf::HitCr) => (false, CrLf::HitLf),
                    (b'.', CrLf::HitLf) => (true, CrLf::None),
                    (_, CrLf::None) => (false, CrLf::None),
                    // this _could_ be invalid data but legacy systems _should_
                    // be able to handle orphan '\r'/'\n' so treat it as ok
                    (_, _) => (false, CrLf::None)
                };
                state = new_state;
                if stash {
                    out.put_u8(b'.');
                }
                out.put_u8(bch);
            }
        }
        self.stash_state = state;
    }
}

impl<S> Future for DotStashedWrite<S>
    where S: Stream<Error=std_io::Error>, S::Item: Buf
{
    type Item = Io;
    type Error = std_io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            //TODO the think below is needed so to handle put wrt. buffer capacity (it panics
            // if it rans out of capacity)
            //TODO this can be improved to not flush each slice befor dot-stashing the next slice
            // e.g. while buffer has space write dot stashed bytes from self.pending into
            // out buffer while poll_flush is NotReady
            try_ready!(self.io_mut().poll_flush());

            if self.write_eom_seq {
                return Ok(Async::Ready(self.io.take().expect("poll after completion")));
            }

            let pending =
                match try_ready!(self.poll_source()) {
                    Some(p) => p,
                    None => continue
                };

            self.write_dot_stashed_output(pending);
        }
    }
}
