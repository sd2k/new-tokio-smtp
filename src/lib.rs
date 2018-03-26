#[macro_use]
extern crate futures;
extern crate bytes;
extern crate tokio;
extern crate tokio_tls;
extern crate native_tls;

pub mod response;
pub mod command;
pub mod io;
mod ehlo_data;

pub use self::ehlo_data::EhloData;

use std::{io as std_io};
use bytes::{BytesMut, BufMut};
use futures::Future;
use self::io::{Io, SmtpResult};

pub type CmdFuture = Box<Future<Item=(Connection, SmtpResult), Error=std_io::Error>>;

pub struct Connection {
    io: Io,
    ehlo: Option<EhloData>,
}


impl Connection {

    pub fn cmd<C: Cmd>(self, cmd: C) -> CmdFuture {
        cmd.exec(self)
    }

    pub fn simple_cmd<C: SimpleCmd>(self, cmd: C) -> CmdFuture {
        let (mut io, ehlo) = self.destruct();
        {
            let buffer = io.out_buffer();
            cmd.write_cmd(buffer);
            buffer.put("\r\n");
        }

        let fut = io
            .flush()
            .and_then(Io::parse_response)
            .map(|(io, response)| (Self::from((io, ehlo)), response));

        Box::new(fut)
    }

    pub fn destruct(self) -> (Io, Option<EhloData>) {
        let Connection { io, ehlo } = self;
        (io, ehlo)
    }
}

impl From<(Io, Option<EhloData>)> for Connection {
    fn from((io, ehlo): (Io, Option<EhloData>)) -> Self {
        Connection { io, ehlo }
    }
}


// what kinds of commands are there
// 1. simple commands (MAIL, RCPT)
// 2. commands returning intermediate and then do the sub-conversation (DATA, AUTH)
// 3. commands without a intermediate which still are special (BDAT)
//
// how to handle them:
// 1. just write cmd (inkl. \r\n) and read result
//  1.1. if result is intermediate try use handle_intermediate or error
// 2. BDAT just writes more than just a command with write_cmd
//  2.1. drawback is that a whole BDAT + DATA package has to fit into the buffer
pub trait Cmd {
    fn exec(self, con: Connection) -> CmdFuture;
}


pub trait SimpleCmd {

    /// writes a simple command to the buffer
    ///
    /// The simple command should be a one-line command.
    /// After this function is called through a call to
    /// `Connection::simple_cmd` the `Connection` _will_
    /// write `"\r\n"`.
    ///
    fn write_cmd(&self, buf: &mut BytesMut);
}