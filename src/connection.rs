use std::{io as std_io};

use futures::future::{self, Future};
use tokio::io::{shutdown, Shutdown};

use ::common::EhloData;
use ::error::MissingCapabilities;
use ::io::{Io, SmtpResult, Socket};


pub type CmdFuture = Box<Future<Item=(Connection, SmtpResult), Error=std_io::Error> + Send + 'static>;

#[derive(Debug)]
pub struct Connection {
    io: Io
}


impl Connection {

    pub fn send<C: Cmd>(self, cmd: C) -> CmdFuture {
        cmd.exec(self)
    }

    pub fn send_simple_cmd(self, parts: &[&str]) -> CmdFuture {
        let mut io = self.into_inner();

        io.write_line_from_parts(parts);

        let fut = io
            .flush()
            .and_then(Io::parse_response)
            .map(|(io, response)| (Connection::from(io), response));

        Box::new(fut)
    }

    /// returns true if the capability is known to be supported, false else wise
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
        self.io.has_capability(cap)
    }

    pub fn ehlo_data(&self) -> Option<&EhloData> {
        self.io.ehlo_data()
    }

    pub fn into_inner(self) -> Io {
        let Connection { io } = self;
        io
    }

    pub fn shutdown(self) -> Shutdown<Socket> {
        let io = self.into_inner();
        let (socket, _, _) = io.split();
        shutdown(socket)
    }

    //TODO[rust/impl Trait]: remove boxing
    /// sends Quit to the server and then shuts down the socket
    pub fn quit(self)
        -> future::AndThen<
            CmdFuture,
            Shutdown<Socket>,
            fn((Connection, SmtpResult)) -> Shutdown<Socket>>
    {
        //Note: this has a circular dependency between Connection <-> cmd StartTls/Ehlo which
        // could be resolved using a ext. trait, but it's more ergonomic this way
        use command::Quit;

        self.send(Quit).and_then(|(con, _res)| con.shutdown())
    }
}

impl From<Io> for Connection {
    fn from(io: Io) -> Self {
        Connection { io }
    }
}

impl From<Connection> for Io {
    fn from(con: Connection) -> Self {
        let Connection { io } = con;
        io
    }
}

impl From<Socket> for Connection {
    fn from(socket: Socket) -> Self {
        let io = Io::from(socket);
        Connection { io }
    }
}



pub trait Cmd: 'static {
    fn check_cmd_availability(&self, caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>;

    fn exec(self, con: Connection) -> CmdFuture;

    fn boxed(self) -> BoxedCmd
        where Self: Sized + 'static
    {
        Box::new(Some(self))
    }
}


pub type BoxedCmd = Box<TypeErasableCmd>;

pub trait TypeErasableCmd {

    /// # Panics
    ///
    /// may panic if called after `_only_once_exec` was
    /// called
    fn _check_cmd_availability(&self, caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>;

    /// # Panics
    ///
    /// may panic if called more then once
    /// (but can't accept `self` instead of `&mut self`
    /// as it requires object-safety)
    ///
    fn _only_once_exec(&mut self, con: Connection) -> CmdFuture;
}

impl<C> TypeErasableCmd for Option<C>
    where C: Cmd
{
    fn _check_cmd_availability(&self, caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        let me = self.as_ref().expect("_check_cmd_availability called after _only_onece_exec");
        me.check_cmd_availability(caps)
    }

    fn _only_once_exec(&mut self, con: Connection) -> CmdFuture {
        let me = self.take().expect("_only_once_exec called a second time");
        me.exec(con)
    }
}

impl Cmd for Box<TypeErasableCmd> {

    fn check_cmd_availability(&self, caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        self._check_cmd_availability(caps)
    }

    fn exec(mut self, con: Connection) -> CmdFuture {
        self._only_once_exec(con)
    }
}
