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
//! let mail_data = Mail::new(EncodingRequirement::None, raw_mail.to_owned().into());
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
//!     Connection::connect_send_quit(config2, vec![ mail2 ])
//!         .then(|result| {
//!             let results = match result {
//!                 Ok(results) => results,
//!                 Err((con, mails, results)) => {
//!                     println!("{} mails where not send due too: {}", mails.len(), con);
//!                     results
//!                 }
//!             };
//!             for (idx, result) in results.iter().enumerate() {
//!                 if let Err((_, err)) = result {
//!                     println!("mail nr {} failed to be send: {}", idx, err)
//!                 } else {
//!                     println!("mail nr {} was send", idx)
//!                 }
//!             }
//!             Result::Ok::<(),()>(())
//!         })
//! }));
//!
//! # // some mock-up, for this example to compile
//! # fn mock_connection_config() -> ConnectionConfig<command::AuthPlain>
//! #  { unimplemented!() }
//! # fn mock_run_with_tokio(f: impl Future) { unimplemented!() }
//! ```
//!
use std::{io as std_io};

use futures::future::{self, Either, Future, Loop};
use vec1::Vec1;

use ::{Cmd, Connection};
use ::error::{LogicError, MissingCapabilities, GeneralError};
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
#[derive(Debug, Clone)]
pub struct Mail {
    encoding_requirement: EncodingRequirement,
    mail: Vec<u8>
}

impl Mail {

    /// create a new mail instance given a encoding requirement and a buffer
    ///
    /// The buffer contains the actual mail and is normally a string.
    pub fn new(encoding_requirement: EncodingRequirement, buffer: Vec<u8>) -> Self {
        Mail {
            encoding_requirement, mail: buffer
        }
    }

    /// true if `SMTPUTF8` is required
    pub fn needs_smtputf8(&self) -> bool {
        self.encoding_requirement == EncodingRequirement::Smtputf8
    }

    pub fn encoding_requirement(&self) -> EncodingRequirement {
        self.encoding_requirement
    }

    pub fn into_raw_data(self) -> Vec<u8> {
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

    //FIXME put on_error back in
    /// creates a new connection, sends all mails and then closes the connection
    ///
    /// This will try to send all mails, if sending a mail fails because of `LogicError`
    /// it will still try to send the other mails. If sending a mail fails because
    /// of an I/O-Error causing the connection to be lost the not send mails are
    /// returned. Through currently the mail which caused the failure will be lost
    /// (which is bad as it's more likely the environment which caused the failure)
    ///
    /// Take a look at the `send_mail` module documentation for an usage example.
    pub fn connect_send_quit<A>(
        config: ConnectionConfig<A>,
        mails: Vec<MailEnvelop>,
    ) -> impl Future<
        Item=Vec<MailSendResult>,
        Error=(GeneralError, Vec<MailEnvelop>, Vec<MailSendResult>)
    > + Send
        where A: Cmd
    {
        //stackify
        let mut mails = mails;
        mails.reverse();

        let mresults: Vec<MailSendResult> = Vec::new();

        let fut = Connection::connect(config)
            .then(|result| match result {
                Err(err) => Err((GeneralError::from(err), mails, mresults)),
                Ok(con) => Ok((con, mails, mresults))
            })
            .and_then(move |(con, mails, mresults)| future::loop_fn((con, mails, mresults),
                |(con, mut mails, mut mresults)| {
                    if let Some(mail) = mails.pop() {
                        let fut = con
                            .send_mail(mail)
                            .then(|result| match result {
                                Ok((con, mail_send_result)) => {
                                    mresults.push(mail_send_result);
                                    Ok(Loop::Continue((con, mails, mresults)))
                                },
                                Err(io_error) => {
                                    Err((GeneralError::from(io_error), mails, mresults))
                                }
                            });

                        Either::A(fut)
                    } else {
                        Either::B(future::ok(Loop::Break((con, mresults))))
                    }
                }
            ))
            .and_then(|(con, results)| {
                con.quit().then(|_| Ok(results))
            });

        fut
    }
}