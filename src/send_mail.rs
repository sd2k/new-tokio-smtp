//! [feature: `send-mail`] provides the send_mail functionality
//!
//! Send mail is a thin abstractions around sending commands,
//! which combines the sending of the `MAIL`, `RCPT`, `DATA`
//! commands with knowledge about wether or not `SMTPUTF8`
//! needs to be used.
//!
//! # Example
//!
//! ```no_run
//! # extern crate futures;
//! # #[macro_use] extern crate new_tokio_smtp;
//! # #[macro_use] extern crate vec1;
//! # use new_tokio_smtp::command;
//! use std::iter::once as one;
//! use futures::stream::{self, Stream};
//! use futures::future::{self, lazy, Future};
//! use new_tokio_smtp::error::GeneralError;
//! use new_tokio_smtp::{Connection, ConnectionConfig};
//! use new_tokio_smtp::send_mail::{
//!     Mail, EncodingRequirement,
//!     MailAddress, MailEnvelop
//! };
//!
//! let config = mock_connection_config();
//!
//! let raw_mail = concat!(
//!     "Date: Thu, 14 Jun 2018 11:22:18 +0000\r\n",
//!     "From: <no-reply@test.test>\r\n",
//!     "\r\n",
//!     "...\r\n"
//! );
//!
//! // this normally adapts to a higher level abstraction
//! // of mail then this crate provides
//! let mail_data = Mail::new(EncodingRequirement::None, raw_mail.to_owned());
//! // the from_unchecked normally can be used if we know the address is valid
//! // a mail address parser will be added at some point in the future
//! let sender = MailAddress::from_unchecked("test@sender.test");
//! let send_to = MailAddress::from_unchecked("test@receiver.test");
//! let mail = MailEnvelop::new(sender, vec1![ send_to ], mail_data);
//!
//! let mail2 = mail.clone();
//! let config2 = config.clone();
//!
//! mock_run_with_tokio(lazy(move || {
//!     Connection::connect(config)
//!         .map_err(GeneralError::from)
//!         .and_then(|con| con.send_mail(mail).map_err(Into::into))
//!         .and_then(|(con, mail_result)| {
//!             if let Err((idx, err)) = mail_result {
//!                 println!("sending mail failed: {}", err)
//!             }
//!             con.quit().map_err(Into::into)
//!         }).then(|res|{
//!             match res {
//!                 Ok(_) => println!("done, and closed connection"),
//!                 Err(err) => println!("problem with connection: {}", err)
//!             }
//!             Result::Ok::<(),()>(())
//!         })
//! }));
//!
//! // This is only overhead as we skipped any (fallible) mail encoding step
//! let mail2: Result<_, GeneralError> = Ok(mail2);
//!
//! //or simpler
//! mock_run_with_tokio(lazy(move || {
//!     // it accepts a iterator over mails,
//!     Connection::connect_send_quit(config2, one(mail2))
//!         //Stream::for_each is conceptually broken in futures v0.1
//!         .then(|res| Ok(res))
//!         .for_each(|result| {
//!             if let Err(err) = result {
//!                 println!("sending mail failed: {}", err);
//!             } else {
//!                 println!("successfully send mail")
//!             }
//!             Ok(())
//!         })
//! }));
//!
//! # // some mock-up, for this example to compile
//! # fn mock_connection_config() -> ConnectionConfig<command::auth::Plain>
//! #  { unimplemented!() }
//! # fn mock_run_with_tokio(f: impl Future<Item=(), Error=()>) { unimplemented!() }
//! ```
//!
use std::io as std_io;
use std::mem::replace;

use bytes::Bytes;
use futures::future::{self, Either, Future};
use futures::stream::Stream;
use futures::{Async, IntoFuture, Poll};
use vec1::Vec1;

use crate::{
    chain::{chain, HandleErrorInChain, OnError},
    command::{self, params_with_smtputf8},
    common::SetupTls,
    connect::ConnectionConfig,
    data_types::{ForwardPath, ReversePath},
    error::{GeneralError, LogicError, MissingCapabilities},
    {Cmd, Connection},
};

/// Specifies if the mail requires SMTPUTF8 (or Mime8bit)
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EncodingRequirement {
    None,
    Smtputf8,
    Mime8bit,
}

/// A simplified representation of a mail consisting of an `EncodingRequirement` and a buffer
///
/// Note that the mail data will be placed internally inside a Bytes instance.
/// Which means it can easily be promoted to an `Arc` if e.g. cloned allowing
/// cheaper clone. The need for this arises
/// from the fact that many smtp applications might want to implement
/// retry logic. E.g. if the connection is interrupted you might want
/// to retry sending the mail once the connection is back etc.
///
#[derive(Debug, Clone)]
pub struct Mail {
    encoding_requirement: EncodingRequirement,
    mail: Bytes,
}

impl Mail {
    /// create a new mail instance given a encoding requirement and a buffer
    ///
    /// The buffer contains the actual mail and is normally a string.
    pub fn new(encoding_requirement: EncodingRequirement, buffer: impl Into<Bytes>) -> Self {
        Mail {
            encoding_requirement,
            mail: buffer.into(),
        }
    }

    /// true if `SMTPUTF8` is required
    pub fn needs_smtputf8(&self) -> bool {
        self.encoding_requirement == EncodingRequirement::Smtputf8
    }

    pub fn encoding_requirement(&self) -> EncodingRequirement {
        self.encoding_requirement
    }

    pub fn raw_data(&self) -> &[u8] {
        self.mail.as_ref()
    }

    pub fn into_raw_data(self) -> Bytes {
        self.mail
    }
}

/// POD representing the smtp envelops from,to's
#[derive(Debug, Clone)]
pub struct EnvelopData {
    /// the sender, this can be `None` i.e. a `<>` reverse path
    pub from: Option<MailAddress>,
    /// the receiver to use with `RCPT TO:`
    pub to: Vec1<MailAddress>,
}

impl EnvelopData {
    /// true if any mail address is a internationalized one
    pub fn needs_smtputf8(&self) -> bool {
        self.from
            .as_ref()
            .map(|f| f.needs_smtputf8())
            .unwrap_or(false)
            || self.to.iter().any(|to| to.needs_smtputf8())
    }
}

/// represents a mail envelop consisting of `EnvelopData` and a `Mail`
#[derive(Debug, Clone)]
pub struct MailEnvelop {
    envelop_data: EnvelopData,
    mail: Mail,
}

impl MailEnvelop {
    //// create a new envelop
    pub fn new(from: MailAddress, to: Vec1<MailAddress>, mail: Mail) -> Self {
        MailEnvelop {
            envelop_data: EnvelopData {
                from: Some(from),
                to,
            },
            mail,
        }
    }

    /// create a envelop with an empty reverse path
    pub fn without_reverse_path(to: Vec1<MailAddress>, mail: Mail) -> Self {
        MailEnvelop {
            envelop_data: EnvelopData { from: None, to },
            mail,
        }
    }

    pub fn from_address(&self) -> Option<&MailAddress> {
        self.envelop_data.from.as_ref()
    }

    pub fn to_address(&self) -> &Vec1<MailAddress> {
        &self.envelop_data.to
    }

    pub fn mail(&self) -> &Mail {
        &self.mail
    }

    /// true if any mail address is internationalized or the mail body needs it
    pub fn needs_smtputf8(&self) -> bool {
        self.envelop_data.needs_smtputf8() || self.mail.needs_smtputf8()
    }
}

impl From<(Mail, EnvelopData)> for MailEnvelop {
    fn from((mail, envelop_data): (Mail, EnvelopData)) -> Self {
        MailEnvelop { envelop_data, mail }
    }
}

impl From<MailEnvelop> for (Mail, EnvelopData) {
    fn from(me: MailEnvelop) -> Self {
        let MailEnvelop { mail, envelop_data } = me;
        (mail, envelop_data)
    }
}

/// A simple `MailAddress` type
///
/// In difference to `ForwardPath` and `ReversePath` this is only a mail
/// address and no other "path" parts. Which is how the paths are mostly
/// used today anyway.
///
/// This type also keeps track of wether or not `SMTPUTF8` is required.
///
/// # Temporary Limitations
///
/// Currently this type doesn't has a mail address parser, once I find
/// a good crate for this it will be included. I.e. currently you
/// have to make sure you mail is valid and then use `from_unchecked`
/// to crate a `MailAddress`, this will also check if it's an internationalized
/// mail address as it can do so without needing to check the grammar.
#[derive(Debug, Clone)]
pub struct MailAddress {
    //FIXME[dep/good mail address crate]: use that
    raw: String,
    needs_smtputf8: bool,
}

impl MailAddress {
    /// create a new `MailAddress` from parts
    ///
    /// this methods relies on the given values to be correct if
    /// the `raw_mail` is actually an internationalized mail address
    /// but `needs_smtputf8` is false this can lead to problems up to
    /// a disconnection of the server (especially if it's an old one)
    pub fn new_unchecked(raw_email: String, needs_smtputf8: bool) -> Self {
        MailAddress {
            raw: raw_email,
            needs_smtputf8,
        }
    }

    /// create a mail from a string not checking syntactical validity
    ///
    /// (through it does check if it's an internationalized mail address)
    pub fn from_unchecked<I>(raw: I) -> Self
    where
        I: Into<String> + AsRef<str>,
    {
        let has_utf8 = raw.as_ref().bytes().any(|b| b >= 0x80);

        MailAddress {
            raw: raw.into(),
            needs_smtputf8: has_utf8,
        }
    }

    pub fn needs_smtputf8(&self) -> bool {
        self.needs_smtputf8
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl AsRef<str> for MailAddress {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Into<String> for MailAddress {
    fn into(self) -> String {
        self.raw
    }
}

impl From<MailAddress> for ReversePath {
    fn from(addr: MailAddress) -> ReversePath {
        ReversePath::from_unchecked(addr.raw)
    }
}

impl From<MailAddress> for ForwardPath {
    fn from(addr: MailAddress) -> ForwardPath {
        ForwardPath::from_unchecked(addr.raw)
    }
}

//IMPROVED maybe return some, all? responses
/// The result of sending a mail
///
/// This is either `()` meaning it succeeded or
/// a tuple of the index of the command which failed
/// and the error with witch it failed. (Detecting that
/// the server does not support SMTPUTF8 but it being required
/// will fail "one the first command", i.e. index 0).
///
pub type MailSendResult = Result<(), (usize, LogicError)>;

/// Future returned by `send_mail`
pub type MailSendFuture =
    Box<dyn Future<Item = (Connection, MailSendResult), Error = std_io::Error> + Send>;

/// Sends a mail specified through `MailEnvelop` through the connection `con`.
///
/// `on_error` is passed to the internally used `chain` and can allow failing
/// some, but not all, `RCPT TO:` commands. Use `chain::OnError::StopAndReset`
/// if you are not sure what to use here.
pub fn send_mail<H>(
    con: Connection,
    envelop: MailEnvelop,
    on_error: H,
) -> impl Future<Item = (Connection, MailSendResult), Error = std_io::Error> + Send
where
    H: HandleErrorInChain,
{
    let use_smtputf8 = envelop.needs_smtputf8();
    let (mail, EnvelopData { from, to: tos }) = envelop.into();

    let check_mime_8bit_support =
        !use_smtputf8 && mail.encoding_requirement() == EncodingRequirement::Mime8bit;

    if (use_smtputf8 && !con.has_capability("SMTPUTF8"))
        || (check_mime_8bit_support && !con.has_capability("8BITMIME"))
    {
        return Either::B(future::ok((
            con,
            Err((
                0,
                MissingCapabilities::new_from_unchecked("SMTPUTF8").into(),
            )),
        )));
    }

    let reverse_path = from
        .map(ReversePath::from)
        .unwrap_or_else(|| ReversePath::from_unchecked(""));

    let mut mail_params = Default::default();
    if use_smtputf8 {
        mail_params = params_with_smtputf8(mail_params);
    }
    let mut cmd_chain = vec![command::Mail {
        reverse_path,
        params: mail_params,
    }
    .boxed()];

    for to in tos.into_iter() {
        cmd_chain.push(command::Recipient::new(to.into()).boxed());
    }

    cmd_chain.push(command::Data::from_buf(mail.into_raw_data()).boxed());

    Either::A(chain(con, cmd_chain, on_error))
}

impl Connection {
    /// Sends a mail specified through `MailEnvelop` through this connection.
    ///
    /// If any command fails sending is stopped and `RSET` is send to the server
    /// to reset the current mail transaction.
    ///
    /// see the module level documentation/README or example dir for example about
    /// how to use this.
    pub fn send_mail(
        self,
        envelop: MailEnvelop,
    ) -> impl Future<Item = (Connection, MailSendResult), Error = std_io::Error> + Send {
        send_mail(self, envelop, OnError::StopAndReset)
    }

    /// Sends all mails from mails through the connection.
    ///
    /// The connection is moved into the `SendAllMails` adapter
    /// and can be retrieved from there.
    ///
    /// Alternatively `SendAllMails.quit_on_completion`
    /// can be used to make the adapter call quite once
    /// all mails are send.
    ///
    /// Or `SendAllMails.on_completion` can be used if
    /// you need to do something else with the same connection
    /// (like putting it back into a connection pool).
    pub fn send_all_mails<E, M>(
        con: Connection,
        mails: M,
        //FIXME[futures/v>=2.0] use Never instead of ()
    ) -> SendAllMails<M>
    where
        E: From<GeneralError>,
        M: Iterator<Item = Result<MailEnvelop, E>>,
    {
        SendAllMails::new(con, mails)
    }

    /// Creates a new connection, sends all mails and then closes the connection
    ///
    /// - if sending a mail fails because of `LogicError` it will still try to send the other mails.
    /// - If sending a mail fails because of an I/O-Error causing the connection to be lost the remaining
    ///   Mails will fail with `GeneralError::Io` with an `std::io::ErrorKind::NoConnection` error.
    ///
    /// This function accepts an `IntoIterable` (instead of a `Stream`) as all mails
    /// should already be available when the connection os opened.
    /// It also expects `Result`'s instead of just mails, as mails normally have to
    /// be encoded which can fail (and is not part of the crate). With this its easier
    /// to adapt it to functionality which e.g. takes a vector of data and creates and
    /// sends mails from it returning a vector of results. Be aware of `std::iter::once`
    /// which provides a handy way to just pass in a single mail envelop.
    ///
    ///
    /// As any future/stream this has to be polled to drive it to completion,
    /// i.e. even if you don't care about the results you have to poll the
    /// future and then the stream it resolves to.
    ///
    /// # Example (where to find it)
    ///
    /// Take a look at the `send_mail` module documentation for an usage example.
    ///
    /// To send a single mail `std::iter::once as one` can be used:
    ///
    /// `Connection::connect_send_quit(config, one(mail))`
    ///
    /// To get back a `Vec` of results you can use:
    ///
    /// `stream.then(|result| Ok(result)).collect()`
    ///
    /// Which is only needed as `futures v0.1` `Stream::collect` method is
    /// conceptually broken. (`Stream`'s are a sequence of results in futures,
    /// which continuos independent of any error result, but `collect` is written
    /// as if streams short circuit once a error is it which is just wrong.)
    ///
    /// ```no_run
    /// # extern crate futures;
    /// # extern crate new_tokio_smtp;
    /// use std::iter::once as one;
    /// # use futures::{stream, Future, Stream};
    /// # use new_tokio_smtp::{Connection, ConnectionConfig, command};
    /// # use new_tokio_smtp::error::GeneralError;
    /// # use new_tokio_smtp::send_mail::MailEnvelop;
    /// # let config: ConnectionConfig<command::Noop> = unimplemented!();
    /// # let mail: Result<MailEnvelop, GeneralError> = unimplemented!();
    /// # // We only have this overhead as we skipped any (fallible) mail encoding process
    /// // note that the map_err is only needed as `!` isn't stable yet
    /// let fut = Connection::connect_send_quit(config, one(mail))
    ///     //Stream::collect is conceptually broken in futures v0.1
    ///     .then(|res| Result::Ok::<_, ()>(res))
    ///     .collect();
    /// # let _ = fut;
    /// ```
    ///
    pub fn connect_send_quit<A, E, I, T>(
        config: ConnectionConfig<A, T>,
        mails: I,
    ) -> impl Stream<Item = (), Error = E>
    where
        A: Cmd,
        E: From<GeneralError>,
        I: IntoIterator<Item = Result<MailEnvelop, E>>,
        T: SetupTls,
    {
        let fut = Connection::connect(config)
            .then(|res| match res {
                Err(err) => Err(E::from(GeneralError::from(err))),
                Ok(con) => Ok(SendAllMails::new(con, mails).quit_on_completion()),
            })
            .flatten_stream();

        fut
    }
}

/// Adapter to send all mails from an iterable instance through a smtp connection.
pub struct SendAllMails<I> {
    mails: I,
    con: Option<Connection>,
    //FIXME[rust/impl Trait in struct]
    pending:
        Option<Box<dyn Future<Item = (Connection, MailSendResult), Error = std_io::Error> + Send>>,
}

impl<I, E> SendAllMails<I>
where
    I: Iterator<Item = Result<MailEnvelop, E>>,
    E: From<GeneralError>,
{
    /// create a new `SendAllMails` stream adapter
    pub fn new<V>(con: Connection, mails: V) -> Self
    where
        V: IntoIterator<IntoIter = I, Item = Result<MailEnvelop, E>>,
    {
        SendAllMails {
            mails: mails.into_iter(),
            con: Some(con),
            pending: None,
        }
    }

    /// takes the connection out of the adapter
    ///
    /// - if there currently is a pending future this will always be `None`
    /// - if `mails` is not completed and this adapter is polled afterwards
    ///   all later mails will fail with `M::Error::from(GeneralError::PreviousErrorKilledConnection)`
    pub fn take_connection(&mut self) -> Option<Connection> {
        self.con.take()
    }

    /// sets the connection to use in the adapter for sending mails
    ///
    /// returns the currently set connection, if any
    pub fn set_connection(&mut self, con: Connection) -> Option<Connection> {
        ::std::mem::replace(&mut self.con, Some(con))
    }

    /// true if a mail is currently in the process of being send
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Quits the contained connection once the stream is completed.
    ///
    /// The result from quitting is discarded, which is fine as this
    /// only happens if:
    ///
    /// 1. for some reason the connection was interrupted (the server already quit)
    /// 2. the server responds with a error to sending the QUIT command
    ///
    /// In both cases it's reasonable to simply drop the connection when
    /// dropping this stream.
    pub fn quit_on_completion(self) -> impl Stream<Item = (), Error = E> {
        OnCompletion::new(self, |stream| {
            if let Some(con) = stream.take_connection() {
                Either::A(con.quit().then(|_| Ok(())))
            } else {
                Either::B(future::ok(()))
            }
        })
    }

    /// Calls a closure once the stream completed with the connection (if there is one).
    ///
    /// The closure can resolve to a future which is resolved, but the result of
    /// the future is ignored.
    ///
    /// A common think to do once the `SendAllFuture` is done is to quit
    /// the connection, through for this `quit_on_completion` should be used.
    /// Another possibility is that if you have a pool of connections the
    /// closure will put the connection back into the pool it took it out
    /// from to allow connection reuse.
    //FIXME[futures/v>=0.2] use Never for IntoFuture futures Error
    pub fn on_completion<F, ITF>(self, func: F) -> impl Stream<Item = (), Error = E>
    where
        F: FnOnce(Option<Connection>) -> ITF,
        ITF: IntoFuture<Item = (), Error = ()>,
    {
        OnCompletion::new(self, |stream| {
            let opt_con = stream.take_connection();
            func(opt_con)
        })
    }
}

impl<I, E> Stream for SendAllMails<I>
where
    I: Iterator<Item = Result<MailEnvelop, E>>,
    E: From<GeneralError>,
{
    type Item = ();
    type Error = E;

    //FIXME[futures/async streams]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            if let Some(mut pending) = self.pending.take() {
                return match pending.poll() {
                    Ok(Async::NotReady) => {
                        self.pending = Some(pending);
                        Ok(Async::NotReady)
                    }
                    Ok(Async::Ready((con, result))) => {
                        self.con = Some(con);
                        match result {
                            Ok(res) => Ok(Async::Ready(Some(res))),
                            Err((_idx, err)) => Err(E::from(GeneralError::from(err))),
                        }
                    }
                    Err(io_error) => Err(E::from(GeneralError::from(io_error))),
                };
            }

            return match self.mails.next() {
                None => Ok(Async::Ready(None)),
                Some(Ok(mail)) => {
                    if let Some(con) = self.con.take() {
                        self.pending = Some(Box::new(con.send_mail(mail)));
                        continue;
                    } else {
                        Err(E::from(GeneralError::Io(std_io::Error::new(
                            std_io::ErrorKind::NotConnected,
                            "previous error killed connection",
                        ))))
                    }
                }
                Some(Err(err)) => Err(err),
            };
        }
    }
}

/// Stream adapt resolving one function/future after the stream completes
///
/// If `S` is fused calling the stream adapter after completion is fine,
/// through the function will only run the time it completes. I.e. if
/// `S` restarts after completion `func` _will not_ be called a second
/// time when it completes again
pub struct OnCompletion<S, F, UF> {
    stream: S,
    state: CompletionState<F, UF>,
}

enum CompletionState<F, U> {
    Done,
    Ready(F),
    Pending(U),
}

impl<F, U> CompletionState<F, U> {
    /// # Panic
    ///
    /// panics if the state is `Pending`
    fn take_func(&mut self) -> Option<F> {
        use self::CompletionState::*;

        let me = replace(self, CompletionState::Done);
        match me {
            Done => None,
            Ready(func) => Some(func),
            Pending(_) => panic!("[BUG] take func in pending state"),
        }
    }
}

impl<S, F, U> OnCompletion<S, F, U::Future>
//FIXME[futures/v>=0.2] Error=Never
where
    S: Stream,
    F: FnOnce(&mut S) -> U,
    U: IntoFuture<Item = (), Error = ()>,
{
    /// creates a new adapter calling func the first time the stream completes.
    ///
    /// When the underlying stream completes func is called and the value returned
    /// by func is turned into a future, while the future is polled the stream is
    /// `Async::NotReady` and once it resolves the stream will return `Async::Ready(None)`,
    /// i.e. it will complete.
    ///
    /// Note that the return value of the future is completely ignored, independent
    /// of wether or not it's resolves to an item or an error the value it resolves
    /// to is just dropped.
    ///
    /// If the stream is fused polling the adapter after completion it is fine too.
    ///
    pub fn new(stream: S, func: F) -> Self {
        OnCompletion {
            stream,
            state: CompletionState::Ready(func), //, _u: ::std::marker::PhantomData
        }
    }
}

impl<S, F, U> Stream for OnCompletion<S, F, U::Future>
//FIXME[futures/v>=0.2] Error=Never
where
    S: Stream,
    F: FnOnce(&mut S) -> U,
    U: IntoFuture,
{
    type Item = S::Item;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let is_done = if let &mut CompletionState::Pending(ref mut fut) = &mut self.state {
                if let Ok(Async::NotReady) = fut.poll() {
                    return Ok(Async::NotReady);
                } else {
                    true
                }
            } else {
                false
            };

            if is_done {
                self.state = CompletionState::Done;
                return Ok(Async::Ready(None));
            }

            let next = try_ready!(self.stream.poll());

            if let Some(next) = next {
                return Ok(Async::Ready(Some(next)));
            } else if let Some(func) = self.state.take_func() {
                let fut = func(&mut self.stream).into_future();
                self.state = CompletionState::Pending(fut);
                continue;
            } else {
                // polled after completion, through maybe S was fused so
                // just return None
                return Ok(Async::Ready(None));
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{
        command, error::GeneralError, send_mail::MailEnvelop, Connection, ConnectionConfig,
    };

    fn assert_send(_: &impl Send) {}

    #[allow(unused)]
    fn assert_send_in_send_out() {
        let config: ConnectionConfig<command::Noop> = unimplemented!();
        let mails: Vec<Result<MailEnvelop, GeneralError>> = unimplemented!();
        assert_send(&mails);
        let fut = Connection::connect_send_quit(config, mails);
        assert_send(&fut);
    }
}
