#[macro_use]
extern crate futures;
extern crate bytes;
extern crate tokio;
extern crate tokio_tls;
extern crate native_tls;

mod future_ext;
mod ascii;
#[macro_use]
mod utils;
mod common;
pub mod response;
pub mod io;
pub mod command;

pub use self::common::*;
pub use self::io::Io;
pub use self::response::Response;

use std::{io as std_io};
use bytes::{BytesMut, BufMut};
use futures::Future;
use self::io::SmtpResult;

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
            let buffer = io.out_buffer(1024);
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
    fn boxed(self) -> BoxedCmd
        where Self: Sized + 'static
    {
        Box::new(Some(self))
    }
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

pub type BoxedCmd = Box<TypeErasableCmd>;

pub trait TypeErasableCmd {
    /// # Panics
    ///
    /// panics if called more then once
    /// (but can't accept `self` instead of `&mut self`
    /// as it requires object-safety)
    ///
    fn _only_once_exec(&mut self, con: Connection) -> CmdFuture;
}

impl<C> TypeErasableCmd for Option<C>
    where C: Cmd
{
    fn _only_once_exec(&mut self, con: Connection) -> CmdFuture {
        let me = self.take().expect("_only_once_exec called a second time");
        me.exec(con)
    }
}

impl Cmd for Box<TypeErasableCmd> {

    fn exec(mut self, con: Connection) -> CmdFuture {
        self._only_once_exec(con)
    }
}
