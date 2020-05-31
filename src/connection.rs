use std::io as std_io;

use futures::future::{self, Either, Future};
use tokio::io::{shutdown, Shutdown};

use common::EhloData;
use error::{LogicError, MissingCapabilities};
use io::{Io, SmtpResult, Socket};

/// future returned by `Cmd::exec`
pub type ExecFuture = Box<Future<Item = (Io, SmtpResult), Error = std_io::Error> + Send + 'static>;

/// The basic `Connection` type representing an (likely) open smtp connection
///
/// It's only likely open as the server could disconnect at any time. But it
/// guaranteed that the last time a command was send over the server did respond
/// with a valid smtp response (through not necessary with a successful one,
/// e.g. the mailbox from a MAIL command might have been rejected or similar)
///
/// Normally the only think done with this type is to construct it with
/// the `connect` method, call the `send` method or the `quit` method (
/// or the `send_mail` cmd if the future is enabled). All other methods
/// of it are mainly for implementor of the `Cmd` trait.
#[derive(Debug)]
pub struct Connection {
    io: Io,
}

impl Connection {
    /// send a command to the smtp server
    ///
    /// This consumes the connection (as it might be modified, recrated or
    /// killed by the command) and returns a future resolving to the result
    /// of sending the command.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # extern crate futures;
    /// # extern crate new_tokio_smtp;
    /// use futures::future::{self, Future};
    /// use new_tokio_smtp::{command, Connection, ReversePath, ForwardPath};
    ///
    ///
    /// let fut = future
    ///     ::lazy(|| mock_create_connection())
    ///     .and_then(|con| {
    ///         con.send(command::Mail::new(
    ///             ReversePath::from_unchecked("test@sender.test")))
    ///     })
    ///     .and_then(|(con, smtp_result)| {
    ///         // using `ctx_and_then`, or `chain` from would make
    ///         // thinks more simple (`future_ext::ResultWithContextExt`)
    ///         if let Err(err) = smtp_result {
    ///             panic!("server says no {}", err)
    ///         }
    ///         con.send(command::Recipient::new(
    ///             ForwardPath::from_unchecked("test@receiver.test")))
    ///     })
    ///     .and_then(|(con, smtp_result)| {
    ///         if let Err(err) = smtp_result {
    ///             panic!("server says no {}", err)
    ///         }
    ///         con.send(command::Data::from_buf(concat!(
    ///             "Date: Thu, 14 Jun 2018 11:22:18 +0000\r\n",
    ///             "From: Sendu <test@sender.test>\r\n",
    ///             "\r\n",
    ///             "...\r\n"
    ///         )))
    ///     })
    ///     .and_then(|(con, smtp_result)| {
    ///         if let Err(err) = smtp_result {
    ///             panic!("server says no {}", err)
    ///         }
    ///         con.quit()
    ///     });
    ///
    /// // ... this are tokio using operation make sure there is
    /// //     a running tokio instance/runtime/event loop
    /// mock_run_with_tokio(fut);
    ///
    /// # // some mock-up, for this example to compile
    /// # fn mock_create_connection() -> Result<Connection, ::std::io::Error>
    /// #  { unimplemented!() }
    /// # fn mock_run_with_tokio(f: impl Future) { unimplemented!() }
    /// ```
    ///
    /// # Logic Failure
    ///
    /// A logic failure is a case where the command was successfully send over
    /// smtp and a response was successfully received but the response code
    /// indicates that the command could not be executed on the smtp server.
    /// For example because the mailbox/mail address  was rejected.
    ///
    /// As long as no connection failure happens the returned future will
    /// resolve to an tuble of the (now again usable) `Connection` instance
    /// and a `SmtpResult` which is either a `Response` or a `LogicError`.
    ///
    /// The `ctx_and_then` or the `future_ext::ResultWithContextExt` trait
    /// can be used to chain `send` calls in a way that the next call is only
    /// run if there was no error at all (neither connection nor logic error).
    ///
    /// # Connection Failure
    ///
    /// If the connection fails (e.g. the internet connection is interrupted)
    /// the future will resolve to an `io::Error` and the connection is gone.
    ///
    pub fn send<C: Cmd>(
        self,
        cmd: C,
    ) -> impl Future<Item = (Connection, SmtpResult), Error = std_io::Error> {
        let fut = if let Err(err) = cmd.check_cmd_availability(self.io.ehlo_data()) {
            Either::B(future::ok((
                self,
                Err(LogicError::MissingCapabilities(err)),
            )))
        } else {
            Either::A(
                cmd.exec(self.into())
                    .map(|(io, smtp_res)| (Connection::from(io), smtp_res)),
            )
        };

        fut
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
    where
        C: AsRef<str>,
    {
        self.io.has_capability(cap)
    }

    /// returns a opt. reference to the ehlo data stored from the last ehlo call
    pub fn ehlo_data(&self) -> Option<&EhloData> {
        self.io.ehlo_data()
    }

    /// converts the `Connection` into an `Io` instance
    ///
    /// This is only need when implementing custom `Cmd`'s
    pub fn into_inner(self) -> Io {
        let Connection { io } = self;
        io
    }

    /// shutdown the connection _without_ sending quit
    pub fn shutdown(self) -> Shutdown<Socket> {
        let io = self.into_inner();
        let (socket, _, _) = io.split();
        shutdown(socket)
    }

    /// sends quit to the server and then shuts down the socket
    ///
    /// The socked is shut down independent of wether or not sending
    /// quit failed, while sending quit should not cause any logic
    /// error if it does it's not returned by this method.
    pub fn quit(self) -> impl Future<Item = Socket, Error = std_io::Error> {
        //Note: this has a circular dependency between Connection <-> cmd StartTls/Ehlo which
        // could be resolved using a ext. trait, but it's more ergonomic this way
        use command::Quit;

        self.send(Quit).and_then(|(con, _res)| con.shutdown())
    }
}

/// create a new `Connection` from a `Io` instance
///
/// The `Io` instance _should_ contain a `Socket` which
/// is still alive.
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

/// create a new `Connection` from a `Socket` instance
///
/// The `Socket` instance _should_ contain a socket which
/// is still alive.
impl From<Socket> for Connection {
    fn from(socket: Socket) -> Self {
        let io = Io::from(socket);
        Connection { io }
    }
}

/// Trait implemented by any smtp command
///
/// While it is not object safe on itself using
/// `cmd.boxed()` provides something very similar
/// to trait object.
pub trait Cmd: Send + 'static {
    /// This method is used to verify if the command can be used
    /// for a given connection
    fn check_cmd_availability(&self, caps: Option<&EhloData>) -> Result<(), MissingCapabilities>;

    /// Executes this command on the given connection
    ///
    /// This method should not be called directly, instead it
    /// is called by `Connection.send`. Which calls this method
    /// with two addition:
    ///
    /// 1. send does use `check_cmd_availability`, so `exec` should
    ///    not do so as it's unnecessary
    /// 2. send turns the `Io` instance the returned future resolves to
    ///    back into a `Connection` instance
    fn exec(self, io: Io) -> ExecFuture;

    /// Turns the command into a `BoxedCmd`
    ///
    /// `BoxedCmd` isn't a trait object of `Cmd` but
    /// it's similar to it and implements `Cmd`. Use this
    /// if you would normally use a `Cmd` trait object.
    /// (e.g. to but a number of cmd's in a `Vec`)
    fn boxed(self) -> BoxedCmd
    where
        Self: Sized + 'static,
    {
        Box::new(Some(self))
    }
}

/// A type acting like a `Cmd` trait object
pub type BoxedCmd = Box<TypeErasableCmd + Send>;

/// A alternate version of `Cmd` which is object safe
/// but has methods which can panic if misused.
///
/// This is just an helper to create `BoxedCmd`, i.e.
/// a way to circumvent to object safety problems of `Cmd`
/// without introducing any additional caused of panics,
/// or errors as long an non of the methods of this trait
/// are used directly. **So just ignore this trait**
///
pub trait TypeErasableCmd {
    /// # Panics
    ///
    /// may panic if called after `_only_once_exec` was
    /// called
    #[doc(hidden)]
    fn _check_cmd_availability(&self, caps: Option<&EhloData>) -> Result<(), MissingCapabilities>;

    /// # Panics
    ///
    /// may panic if called more then once
    /// (but can't accept `self` instead of `&mut self`
    /// as it requires object-safety)
    #[doc(hidden)]
    fn _only_once_exec(&mut self, io: Io) -> ExecFuture;
}

#[doc(hidden)]
impl<C> TypeErasableCmd for Option<C>
where
    C: Cmd,
{
    fn _check_cmd_availability(&self, caps: Option<&EhloData>) -> Result<(), MissingCapabilities> {
        let me = self
            .as_ref()
            .expect("_check_cmd_availability called after _only_onece_exec");
        me.check_cmd_availability(caps)
    }

    fn _only_once_exec(&mut self, io: Io) -> ExecFuture {
        let me = self.take().expect("_only_once_exec called a second time");
        me.exec(io)
    }
}

impl Cmd for BoxedCmd {
    fn check_cmd_availability(&self, caps: Option<&EhloData>) -> Result<(), MissingCapabilities> {
        self._check_cmd_availability(caps)
    }

    fn exec(mut self, io: Io) -> ExecFuture {
        self._only_once_exec(io)
    }
}

//FIXME[rustc/specialization]
// impl<T> From<T> for BoxedCmd
//     where T: Cmd
// {
//     fn from(cmd: T) -> Self {
//         cmd.boxed()
//     }
// }
