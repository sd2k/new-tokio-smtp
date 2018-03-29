#[macro_use]
extern crate futures;
extern crate bytes;
extern crate tokio;
extern crate tokio_tls;
extern crate native_tls;

mod future_ext;
mod ascii;
mod common;
#[macro_use]
mod tls_utils;
pub mod response;
pub mod io;
pub mod command;

pub use self::common::*;
pub use self::io::Io;
pub use self::response::Response;
pub use self::tls_utils::{SetupTlsData, SetupTls};

use std::{io as std_io};
use std::net::SocketAddr;

use bytes::{BytesMut, BufMut};
use futures::future::{self, Future};

use self::future_ext::ResultWithContextExt;
use self::io::SmtpResult;

pub type CmdFuture = Box<Future<Item=(Connection, SmtpResult), Error=std_io::Error>>;

pub struct Connection {
    io: Io,
    ehlo: Option<EhloData>,
}


impl Connection {

    //TODO[rust/impl Trait]: remove boxing
    pub fn connect_insecure_no_ehlo(addr: &SocketAddr) -> CmdFuture {
        let fut = Io
            ::connect_insecure(addr)
            .and_then(Io::parse_response)
            .map(|(io, response)| (Connection::from(io), response));

        Box::new(fut)
    }

    //TODO[rust/impl Trait]: remove boxing
    pub fn connect_direct_tls_no_ehlo<S>(how: SetupTlsData<S>)
        -> CmdFuture
        where S: SetupTls
    {
        let fut = Io
            ::connect_secure(how)
            .and_then(Io::parse_response)
            .map(|(io, response)| (Connection::from(io), response));

        Box::new(fut)
    }

    pub fn connect_insecure(addr: &SocketAddr, clid: ClientIdentity) -> CmdFuture {
        //Note: this has a cicular dependency between Connection <-> cmd Ehlo which
        // could be resolved using a ext. trait, but it's more ergonomic this way
        use command::Ehlo;
        let fut = Connection
            ::connect_insecure_no_ehlo(addr)
            .ctx_and_then(move |con, _| {
                con.send(Ehlo::from(clid))
            });


        Box::new(fut)
    }

    pub fn connect_direct_tls<S>(
        how: SetupTlsData<S>,
        clid: ClientIdentity
    ) -> CmdFuture
        where S: SetupTls
    {
        //Note: this has a cicular dependency between Connection <-> cmd Ehlo which
        // could be resolved using a ext. trait, but it's more ergonomic this way
        use command::Ehlo;
        let fut = Connection
            ::connect_direct_tls_no_ehlo(how)
            .ctx_and_then(move |con, _| {
                con.send(Ehlo::from(clid))
            });

        Box::new(fut)
    }

    pub fn connect_starttls<S>(
        how: SetupTlsData<S>,
        clid: ClientIdentity
    )
        -> CmdFuture
        where S: SetupTls
    {
        //Note: this has a cicular dependency between Connection <-> cmd StartTls/Ehlo which
        // could be resolved using a ext. trait, but it's more ergonomic this way
        use command::{StartTls, Ehlo};
        let SetupTlsData { addr, domain, setup } = how;

        let fut = Connection
            ::connect_insecure(&addr, clid.clone())
            .ctx_and_then(|con, _| {
                if !con.has_capability("STARTTLS") {
                    let fut = future::err(std_io::Error::new(
                        std_io::ErrorKind::Other,
                        "server does not support STARTTLS"
                    ));
                    Box::new(fut)
                } else {
                    con.send(StartTls {
                        setup_tls: setup,
                        sni_domain: domain
                    })
                }
            })
            .ctx_and_then(|con, _| {
                con.send(Ehlo::from(clid))
            });

        Box::new(fut)
    }

    pub fn send<C: Cmd>(self, cmd: C) -> CmdFuture {
        cmd.exec(self)
    }

    pub fn send_simple_cmd<C: SimpleCmd>(self, cmd: C) -> CmdFuture {
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

    /// returns true if the capability is known to be supported, false elsewise
    ///
    /// The capability is know to be supported if the connection has EhloData and
    /// it was in the ehlo data (as a ehlo-keyword in one of the ehlo-lines after
    /// the first response line).
    ///
    /// If the connection has no ehlo data or the capability is not in the ehlo
    /// data false is returned.
    pub fn has_capability<C>(&self, cap: C) -> bool
        where C: AsRef<str>
    {
        self.ehlo.as_ref().map(|ehlo| {
            ehlo.has_capability(cap)
        }).unwrap_or(false)
    }

    pub fn ehlo_data(&self) -> Option<&EhloData> {
        self.ehlo.as_ref()
    }

    pub fn destruct(self) -> (Io, Option<EhloData>) {
        let Connection { io, ehlo } = self;
        (io, ehlo)
    }
}

impl From<Io> for Connection {
    fn from(io: Io) -> Self {
        Connection { io, ehlo: None }
    }
}

impl From<(Io, EhloData)> for Connection {
    fn from((io, ehlo): (Io, EhloData)) -> Self {
        Connection { io, ehlo: Some(ehlo) }
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
