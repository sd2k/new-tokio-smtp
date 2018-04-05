use bytes::BytesMut;
use bytes::buf::BufMut;

use tokio_tls::TlsStream;
use tokio::net::TcpStream;

use ::response::Response;


mod socket;
pub use self::socket::*;

mod flush;
pub use self::flush::*;

mod parse_result;
pub use self::parse_result::*;

mod dot_stashing;
pub use self::dot_stashing::*;

mod connect;
pub use self::connect::*;

pub const CR_LF: &str = "\r\n";

// most responses should fit in 256 bytes
const INPUT_BUFFER_INC_SIZE: usize = 256;
// most commands should fit in 1024 bytes (except e.g. DATA/BDAT)
const OUTPUT_BUFFER_INC_SIZE: usize = 1024;

pub type SmtpResult = Result<Response, Response>;


#[derive(Debug)]
pub struct Io {
    socket: Socket,
    buffer: Buffers,
}

impl Io {

    /*
       //---------------------------------------------------------------\\
      || Note: More methods are provided through the io::* submodules    ||
       \\---------------------------------------------------------------//
    */

    pub fn split(self) -> (Socket, Buffers) {
        let Io { socket, buffer } = self;
        (socket, buffer)
    }

    pub fn socket_mut(&mut self) -> &mut Socket {
        &mut self.socket
    }

    pub fn socket(&self) -> &Socket {
        &self.socket
    }

    pub fn is_secure(&self) -> bool {
        self.socket.is_secure()
    }

    pub fn out_buffer(&mut self, need_rem: usize) -> &mut BytesMut {
        let buf = &mut self.buffer.output;
        reverse_buffer_cap(buf, need_rem, OUTPUT_BUFFER_INC_SIZE);
        buf
    }

    pub fn in_buffer(&mut self) -> &mut BytesMut {
        &mut self.buffer.input
    }

}

impl From<(Socket, Buffers)> for Io {
    fn from((socket, buffer): (Socket, Buffers)) -> Self {
        Io { socket, buffer }
    }
}

impl From<TcpStream> for Io {
    fn from(stream: TcpStream) -> Self {
        let socket = Socket::Insecure(stream);
        let buffers = Buffers::new();
        Io::from((socket, buffers))
    }
}

impl From<TlsStream<TcpStream>> for Io {
    fn from(stream: TlsStream<TcpStream>) -> Self {
        let socket = Socket::Secure(stream);
        let buffers = Buffers::new();
        Io::from((socket, buffers))
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

#[inline]
fn reverse_buffer_cap(buf: &mut BytesMut, need_rem: usize, increase: usize) {
    let rem = buf.remaining_mut();
    if rem < need_rem {
        let mut reserve = rem + increase;
        while reserve < need_rem {
            reserve += increase;
        }
        // this will keep the capacity a multiple of increase,
        // at last as long as everyone keeps to this schema
        buf.reserve(reserve)
    }
}