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
//! let sender = MailAddress::from_str_unchecked("test@sender.test");
//! let send_to = MailAddress::from_str_unchecked("test@receiver.test");
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
//! //or simpler (but with more verbose output)
//! mock_run_with_tokio(lazy(move || {
//!     let mails = stream::once(Result::Ok::<_, GeneralError>(mail2));
//!     Connection::connect_send_quit(config2, mails)
//!         .and_then(|results| {
//!             results.for_each(|result| {
//!                 if let Err(err) = result {
//!                     println!("sending mail failed: {}", err);
//!                 } else {
//!                     println!("successfully send mail")
//!                 }
//!                 Ok(())
//!             })
//!             // will be gone once `!` is stable
//!             .map_err(|_| unreachable!())
//!         })
//!         .or_else(|conerr| {
//!             println!("connecting failed: {}", conerr);
//!             Ok(())
//!         })
//! }));
//!
//! # // some mock-up, for this example to compile
//! # fn mock_connection_config() -> ConnectionConfig<command::AuthPlain>
//! #  { unimplemented!() }
//! # fn mock_run_with_tokio(f: impl Future<Item=(), Error=()>) { unimplemented!() }
//! ```
//!
use std::{io as std_io};
use std::mem::replace;

use bytes::Bytes;
use futures::{Poll, Async, IntoFuture};
use futures::future::{self, Either, Future};
use futures::stream::Stream;
use vec1::Vec1;

use ::{Cmd, Connection};
use ::error::{
    LogicError, MissingCapabilities,
    GeneralError, ConnectingFailed
};
use ::chain::{chain, OnError, HandleErrorInChain};
use ::data_types::{ReversePath, ForwardPath};
use ::command::{self, params_with_smtputf8};
use ::connect::ConnectionConfig;

/// Specifies if the mail requires SMTPUTF8 (or Mime8bit)
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EncodingRequirement {
    None,
    Smtputf8,
    Mime8bit
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
    mail: Bytes
}

impl Mail {

    /// create a new mail instance given a encoding requirement and a buffer
    ///
    /// The buffer contains the actual mail and is normally a string.
    pub fn new(encoding_requirement: EncodingRequirement, buffer: impl Into<Bytes>) -> Self {
        Mail {
            encoding_requirement, mail: buffer.into()
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
        self.from.as_ref().map(|f| f.needs_smtputf8()).unwrap_or(false)
            || self.to.iter().any(|to| to.needs_smtputf8())
    }
}

/// represents a mail envelop consisting of `EnvelopData` and a `Mail`
#[derive(Debug, Clone)]
pub struct MailEnvelop {
    envelop_data: EnvelopData,
    mail: Mail
}

impl MailEnvelop {

    //// create a new envelop
    pub fn new(from: MailAddress, to: Vec1<MailAddress>, mail: Mail) -> Self {
        MailEnvelop {
            envelop_data: EnvelopData { from: Some(from), to },
            mail
        }
    }

    /// create a envelop with an empty reverse path
    pub fn without_reverse_path(to: Vec1<MailAddress>, mail: Mail) -> Self {
        MailEnvelop {
            envelop_data: EnvelopData { from: None, to },
            mail
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
    needs_smtputf8: bool
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
            needs_smtputf8
        }
    }

    pub fn from_str_unchecked<I>(raw: I) -> Self
        where I: Into<String> + AsRef<str>
    {
        let has_utf8 = raw.as_ref().bytes().any(|b| b >= 0x80);

        MailAddress {
            raw: raw.into(),
            needs_smtputf8: has_utf8
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
        ReversePath::from_str_unchecked(addr.raw)
    }
}

impl From<MailAddress> for ForwardPath {
    fn from(addr: MailAddress) -> ForwardPath {
        ForwardPath::from_str_unchecked(addr.raw)
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
pub type MailSendFuture = Box<Future<Item=(Connection, MailSendResult), Error=std_io::Error> + Send>;

/// Sends a mail specified through `MailEnvelop` through the connection `con`.
///
/// `on_error` is passed to the internally used `chain` and can allow failing
/// some, but not all, `RCPT TO:` commands. Use `chain::OnError::StopAndReset`
/// if you are not sure what to use here.
pub fn send_mail<H>(con: Connection, envelop: MailEnvelop, on_error: H)
    -> impl Future<Item=(Connection, MailSendResult), Error=std_io::Error> + Send
    where H: HandleErrorInChain
{
    let use_smtputf8 =  envelop.needs_smtputf8();
    let (mail, EnvelopData { from, to: tos }) = envelop.into();

    let check_mime_8bit_support =
        !use_smtputf8 && mail.encoding_requirement() == EncodingRequirement::Mime8bit;

    if (use_smtputf8 && !con.has_capability("SMTPUTF8"))
       || (check_mime_8bit_support && !con.has_capability("8BITMIME"))
    {
        return Either::B(future::ok(
            (con, Err((0, MissingCapabilities::new_from_str_unchecked("SMTPUTF8").into())))
        ));
    }

    let reverse_path = from.map(ReversePath::from)
        .unwrap_or_else(|| ReversePath::from_str_unchecked(""));

    let mut mail_params = Default::default();
    if use_smtputf8 {
        mail_params  = params_with_smtputf8(mail_params);
    }
    let mut cmd_chain = vec![
        //FIXME[BUG] use param SMTPUTF8 if use_smtputf8
        command::Mail {
            reverse_path,
            params: mail_params
        }.boxed()
    ];

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
    pub fn send_mail(self, envelop: MailEnvelop)
        -> impl Future<Item=(Connection, MailSendResult), Error=std_io::Error> + Send
    {
        send_mail(self, envelop, OnError::StopAndReset)
    }

    /// sends all mails from mails through the connection
    ///
    /// The connection is moved into the `SendAllMails` adapter
    /// and can be retrieved from there. Alternatively `quit_on_completion`
    /// can be used to make the adapter call quite once all mails are send.
    pub fn send_all_mails<A, E, M>(
        con: Connection,
        mails: M,
        //FIXME[futures/v>=2.0] use Never instead of ()
    ) -> SendAllMails<M>
        where A: Cmd, E: From<GeneralError>, M: Stream<Item=MailEnvelop, Error=E>
    {
        SendAllMails::new(con, mails)
    }

    //FIXME put on_error back in
    /// creates a new connection, sends all mails and then closes the connection
    ///
    /// - if sending a mail fails because of `LogicError` it will still try to send the other mails.
    /// - If sending a mail fails because of an I/O-Error causing the connection to be lost the remaining
    ///   Mails will fail with `GeneralError::PreviousErrorKilledConnection`.
    ///
    /// This function will poll first open a connection _then_ poll mails from the
    /// `mail` stream sending them through the connection and then close the
    /// connection. As some mail servers cut off unused connections it might
    /// be a good idea to make sure all mails are available when the connection
    /// is opened, i.e. to make sure polling `mails` doesn't have to wait long,
    /// through if this is necessary depends on the mail server/provider.
    ///
    /// As any future/stream this has to be polled to drive it to completion,
    /// i.e. even if you don't care about the results you have to poll the
    /// future and then the stream it resolves to.
    ///
    /// # Example (where to find it)
    ///
    /// Take a look at the `send_mail` module documentation for an usage example.
    ///
    /// To send a number of mails from a vec you can use:
    ///
    /// `Connection::connect_send_quit(config, stream::iter_ok::<_, GeneralError>(vec_of_mails))`
    ///
    /// To get back a `Vec` of results you can use:
    ///
    /// ```no_run
    /// # extern crate futures;
    /// # extern crate new_tokio_smtp;
    /// # use futures::{stream, Future, Stream};
    /// # use new_tokio_smtp::{Connection, ConnectionConfig, command};
    /// # use new_tokio_smtp::error::GeneralError;
    /// # use new_tokio_smtp::send_mail::MailEnvelop;
    /// # let config: ConnectionConfig<command::Noop> = unimplemented!();
    /// # let mail_vec: Vec<MailEnvelop> = unimplemented!();
    /// # let mails = ::futures::stream::iter_ok::<_, GeneralError>(mail_vec.into_iter());
    /// // note that the map_err is only needed as `!` isn't stable yet
    /// let fut = Connection::connect_send_quit(config, mails)
    ///     .and_then(|results| results.collect().map_err(|_| unreachable!()));
    /// # let _ = fut;
    /// ```
    ///
    /// # Design Note
    ///
    /// Note that the implementation intentionally returns a `Item=Result<_,_>, Error=()`
    /// instead of an `Item=_, Error=_` as some combinators do not play well with cases
    /// where the stream represents a sequence of results instead of a sequence of items
    /// where the stream can fail. E.g. `collect` would discard thinks if any mail failed,
    /// which isn't what is expected/wanted at all.
    ///
    ///
    pub fn connect_send_quit<A, E>(
        config: ConnectionConfig<A>,
        mails: impl Stream<Item=MailEnvelop, Error=E>,
        //FIXME[futures/v>=2.0] use Never instead of ()
    ) -> impl Future<Item=impl Stream<Item=Result<(), E>, Error=()>, Error=ConnectingFailed>
        where A: Cmd, E: From<GeneralError>
    {
        let fut = Connection
            ::connect(config)
            .map(|con| {
                OnCompletion::new(
                    SendAllMails::new(con, mails),
                    |send_adapter| {
                        if let Some(con) = send_adapter.take_connection() {
                            Either::A(con.quit().map(|_|()))
                        } else {
                            Either::B(future::ok(()))
                        }
                    }
                )
            });

        fut
    }
}

/// adapter to send a stream of mails through a smtp connection
pub struct SendAllMails<M> {
    mails: M,
    con: Option<Connection>,
    //FIXME[rust/impl Trait in struct]
    pending: Option<Box<Future<Item=(Connection, MailSendResult), Error=std_io::Error> + Send>>,
}

impl<M> SendAllMails<M>
    where M: Stream<Item=MailEnvelop>, M::Error: From<GeneralError>
{
    /// create a new `SendAllMails` stream adapter
    pub fn new(con: Connection, mails: M) -> Self {
        SendAllMails {
            mails,
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
}

impl<M> Stream for SendAllMails<M>
    where M: Stream<Item=MailEnvelop>, M::Error: From<GeneralError>
{
    type Item=Result<(), M::Error>;
    //FIXME[futures/v>=0.2] use Never
    type Error=();

    //FIXME[futures/async streams]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            if let Some(pending) = self.pending.as_mut() {
                return match pending.poll() {
                    Ok(Async::NotReady) => Ok(Async::NotReady),
                    Ok(Async::Ready((con, result))) => {
                        self.con = Some(con);
                        let result = result.map_err(|(_idx, logic_err)| {
                            M::Error::from(GeneralError::from(logic_err))
                        });
                        Ok(Async::Ready(Some(result)))
                    },
                    Err(io_error) => {
                        let err = M::Error::from(GeneralError::from(io_error));
                        Ok(Async::Ready(Some(Err(err))))
                    }
                };
            }

            return match self.mails.poll() {
                Ok(Async::NotReady) => Ok(Async::NotReady),
                Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
                Ok(Async::Ready(Some(mail))) => {
                    if let Some(con) = self.con.take() {
                        self.pending = Some(Box::new(con.send_mail(mail)));
                        continue;
                    } else {
                        let err = M::Error::from(GeneralError::PreviousErrorKilledConnection);
                        Ok(Async::Ready(Some(Err(err))))
                    }
                },
                Err(err) => {
                    Ok(Async::Ready(Some(Err(err))))
                }
            };

        }
    }
}

/// stream adapt resolving one function/future after the stream completes
///
/// If `S` is fused calling the stream adapter after completion is fine,
/// through the function will only run the time it completes. I.e. if
/// `S` restarts after completion `func` _will not_ be called a second
/// time when it completes again
pub struct OnCompletion<S, F, UF> {
    stream: S,
    state: CompletionState<F, UF>
    //_u: ::std::marker::PhantomData<U>
}

enum CompletionState<F, U> {
    Done,
    Ready(F),
    Pending(U)
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
            Pending(_) => panic!("[BUG] take func in pending state")
        }
    }
}

impl<S, F, U> OnCompletion<S, F, U::Future>
    //FIXME[futures/v>=0.2] Error=Never
    where S: Stream, F: FnOnce(&mut S) -> U, U: IntoFuture
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
            stream, state: CompletionState::Ready(func)
            //, _u: ::std::marker::PhantomData
        }
    }
}

impl<S, F, U> Stream for OnCompletion<S, F, U::Future>
    //FIXME[futures/v>=0.2] Error=Never
    where S: Stream, F: FnOnce(&mut S) -> U, U: IntoFuture
{
    type Item = S::Item;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let is_done =
                if let &mut CompletionState::Pending(ref mut fut) = &mut self.state {
                    if let Ok(Async::NotReady) = fut.poll() {
                        return Ok(Async::NotReady)
                    } else {
                        true
                    }
                } else {
                    false
                };

            if is_done {
                self.state = CompletionState::Done;
                return Ok(Async::Ready(None))
            }

            let next = try_ready!(self.stream.poll());

            if let Some(next) = next {
                return Ok(Async::Ready(Some(next)));
            } else if let Some(func) = self.state.take_func() {
                let fut = func(&mut self.stream).into_future();
                self.state = CompletionState::Pending(fut);
                continue
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
    use ::{Connection, ConnectionConfig, command};
    use ::error::GeneralError;
    use ::send_mail::MailEnvelop;

    fn assert_send(_: &impl Send) {}

    #[allow(unused)]
    fn assert_send_in_send_out() {
        let config: ConnectionConfig<command::Noop> = unimplemented!();
        let mail_vec: Vec<MailEnvelop> = unimplemented!();
        let mails = ::futures::stream::iter_ok::<_, GeneralError>(mail_vec.into_iter());
        assert_send(&mails);
        let fut = Connection::connect_send_quit(config, mails);
        assert_send(&fut);
    }

}