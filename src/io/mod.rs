use bytes::BytesMut;
use bytes::buf::BufMut;

use tokio_tls::TlsStream;
use tokio::net::TcpStream;

use ::common::EhloData;
use ::response::Response;
use ::error::LogicError;


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

pub type SmtpResult = Result<Response, LogicError>;


#[derive(Debug)]
pub struct Io {
    socket: Socket,
    buffer: Buffers,
    ehlo_data: Option<EhloData>,
}

impl Io {

    /*
       //---------------------------------------------------------------\\
      || Note: More methods are provided through the io::* submodules    ||
       \\---------------------------------------------------------------//
    */

    pub fn split(self) -> (Socket, Buffers, Option<EhloData>) {
        let Io { socket, buffer, ehlo_data } = self;
        (socket, buffer, ehlo_data)
    }

    /// writes all strings in `parts` to the output buffer followed by `"\r\n"`
    pub fn write_line_from_parts(&mut self, parts: &[&str]) {
        let len = parts
            .iter()
            .fold(CR_LF.len(), |sum, item| sum + item.len());

        let buffer = self.out_buffer(len);
        for part in parts {
            buffer.put(*part);
        }
        buffer.put(CR_LF);
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

    pub fn ehlo_data(&self) -> Option<&EhloData> {
        self.ehlo_data.as_ref()
    }

    pub fn set_ehlo_data(&mut self, data: EhloData) {
        self.ehlo_data = Some(data);
    }

    pub fn has_capability<C>(&self, cap: C) -> bool
        where C: AsRef<str>
    {
        self.ehlo_data().map(|ehlo| {
            ehlo.has_capability(cap)
        }).unwrap_or(false)
    }

}

impl From<(Socket, Buffers, Option<EhloData>)> for Io {
    fn from((socket, buffer, ehlo_data): (Socket, Buffers, Option<EhloData>)) -> Self {
        Io { socket, buffer, ehlo_data }
    }
}

impl From<(Socket, Buffers, EhloData)> for Io {
    fn from((socket, buffer, ehlo_data): (Socket, Buffers, EhloData)) -> Self {
        Io { socket, buffer, ehlo_data: Some(ehlo_data) }
    }
}

impl From<(Socket, Buffers)> for Io {
    fn from((socket, buffer): (Socket, Buffers)) -> Self {
        Io { socket, buffer, ehlo_data: None }
    }
}

impl From<Socket> for Io {
    fn from(socket: Socket) -> Self {
        Io { socket, buffer: Buffers::new(), ehlo_data: None }
    }
}

impl From<TcpStream> for Io {
    fn from(stream: TcpStream) -> Self {
        let socket = Socket::Insecure(stream);
        let buffers = Buffers::new();
        Io::from((socket, buffers, None))
    }
}

impl From<TlsStream<TcpStream>> for Io {
    fn from(stream: TlsStream<TcpStream>) -> Self {
        let socket = Socket::Secure(stream);
        let buffers = Buffers::new();
        Io::from((socket, buffers, None))
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