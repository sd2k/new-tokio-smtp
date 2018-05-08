use std::io as std_io;
use std::fmt::Debug;

use futures::Poll;
use bytes::buf::{Buf, BufMut};
use tokio::net::TcpStream;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tls::TlsStream;

#[derive(Debug)]
pub enum Socket {
    Secure(TlsStream<TcpStream>),
    Insecure(TcpStream),
    #[cfg(feature="mock_support")]
    Mock(Box<MockStream + Send>)
}

impl Socket {

    pub fn is_secure(&self) -> bool {
        match *self {
            Socket::Secure(_) => true,
            Socket::Insecure(_) => false,
            #[cfg(feature="mock_support")]
            Socket::Mock(ref mock) => mock.is_secure()
        }
    }
}

macro_rules! socket_mux {
    ($self:ident, |$socket:ident| $block:block) => ({
        match *$self {
            Socket::Secure(ref mut $socket) => $block,
            Socket::Insecure(ref mut $socket) => $block,
            #[cfg(feature="mock_support")]
            Socket::Mock(ref mut $socket) => $block
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
    #[inline]
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        match *self {
            Socket::Secure(ref socket) => socket.prepare_uninitialized_buffer(buf),
            Socket::Insecure(ref socket) => socket.prepare_uninitialized_buffer(buf),
            #[cfg(feature="mock_support")]
            Socket::Mock(ref socket) => socket.prepare_uninitialized_buffer(buf)
        }
    }

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

pub trait MockStream: Debug + AsyncRead + AsyncWrite + 'static {
    fn is_secure(&self) -> bool {
        false
    }
    fn set_is_secure(&mut self, secure: bool);
}