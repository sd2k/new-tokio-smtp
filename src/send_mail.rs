
/*

Requirements:

    - have some form of Mail type containing:
        1. a optional From (there is the <> reverse path)
        2. one or more To
        3. a mail as Vec<u8>

    - have some form of MailBox type containing:
        1. a mail address, through _not_ a path
        2. do we support address literals?
        3. some indicator that it is a internationalized address
        4. with From/Into Forward and ReversePath

    - some indicator/decision if failing sending a specific To should
      stop sending the mail

    - a decision about the returned type (Vec<Result>?)

    - a extension trait for Connection adding ConnectionExt.send_mail

Implementation:

    - create Vec of commands, then use chain


Problems:

    - writing our own Mailbox typ is bad,
      we probably would want to have a mailbox-type
      crate which does following:
        - parse email addresses given different specs
            - includes (optional?) auto conversion of internationalized domains to puny code
            - yes some spec have to parse comments, comments might have to be accessible
        - have a SpecMailAddress<Spec>(MailAddress, PhantomData<Spec>) and a MailAddress
        - have a MailBox::from_str_unchecked or similar
        - indicates if it's us-ascii or utf-8
        - comparison might be added later but is difficult for a number of reasons
            - some special comparison algorithms like eq_gmail (ignores '.', case and + suffixes)
        - a form of "as_parts" conversion which allow encoding with different schemes
            - e.g. "tom@domain.de" => ("tom", "@", "domain.de") => Text "tom", MarkFWS, Text "@", etc.
            - uh, what about Address literals?

Temporary Solution:

    - use a `Mailbox::from_str_unchecked` or similar?
*/
use std::{io as std_io};

use futures::Future;
use vec1::Vec1;

use ::{Cmd, Connection};
use ::chain::{chain, OnError, HandleErrorInChain};
use ::data_types::{ReversePath, ForwardPath};
use ::command::{self, params_with_smtputf8};
use ::error::{LogicError, MissingCapabilities};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EncodingRequirement {
    None,
    Smtputf8,
    Mime8bit
}

pub struct Mail {
    encoding_requirement: EncodingRequirement,
    mail: Vec<u8>
}

impl Mail {

    pub fn new(encoding_requirement: EncodingRequirement, buffer: Vec<u8>) -> Self {
        Mail {
            encoding_requirement, mail: buffer
        }
    }

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

pub struct MailEnvelop {
    from: Option<MailAddress>,
    to: Vec1<MailAddress>,
    mail: Mail
}

impl MailEnvelop {

    pub fn new(from: MailAddress, to: Vec1<MailAddress>, mail: Mail) -> Self {
        MailEnvelop {
            to, mail,
            from: Some(from)
        }
    }

    pub fn without_reverse_path(to: Vec1<MailAddress>, mail: Mail) -> Self {
        MailEnvelop {
            to, mail,
            from: None
        }
    }

    pub fn from_address(&self) -> Option<&MailAddress> {
        self.from.as_ref()
    }

    pub fn to_address(&self) -> &Vec1<MailAddress> {
        &self.to
    }

    pub fn mail(&self) -> &Mail {
        &self.mail
    }

    pub fn needs_smtputf8(&self) -> bool {
        self.from.as_ref().map(|f| f.needs_smtputf8()).unwrap_or(false)
            || self.to.iter().any(|to| to.needs_smtputf8())
            || self.mail.needs_smtputf8()
    }

    pub fn split(self) -> (Option<MailAddress>, Vec1<MailAddress>, Mail) {
        let MailEnvelop { from, to, mail } = self;
        (from, to, mail)
    }
}

pub struct MailAddress {
    //FIXME[dep/good mail address crate]: use that
    raw: String,
    needs_smtputf8: bool
}

impl MailAddress {

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

pub fn send_mail<H>(con: Connection, envelop: MailEnvelop, on_error: H)
    //TODO better error
    -> Result<
        Box<Future<Item=(Connection, Result<(), (usize, LogicError)>), Error=std_io::Error>>,
        (Connection, MissingCapabilities)>
    where H: HandleErrorInChain + 'static
{
    let use_smtputf8 =  envelop.needs_smtputf8();
    let (from, tos, mail) = envelop.split();
    let check_mime_8bit_support =
        !use_smtputf8 && mail.encoding_requirement() == EncodingRequirement::Mime8bit;

    if use_smtputf8 && !con.has_capability("SMTPUTF8") {
        return Err((con, MissingCapabilities::new_from_str_unchecked("SMTPUTF8")));
    }

    if check_mime_8bit_support && !con.has_capability("8BITMIME") {
        return Err((con, MissingCapabilities::new_from_str_unchecked("SMTPUTF8")));
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

    Ok(chain(con, cmd_chain, on_error))
}


pub trait ConSendMailExt {

    fn send_mail(self, envelop: MailEnvelop)
        -> Result<
            Box<Future<Item=(Connection, Result<(), (usize, LogicError)>), Error=std_io::Error>>,
            (Connection, MissingCapabilities)>;
}

impl ConSendMailExt for Connection {
    fn send_mail(self, envelop: MailEnvelop)
        -> Result<
            Box<Future<Item=(Connection, Result<(), (usize, LogicError)>), Error=std_io::Error>>,
            (Connection, MissingCapabilities)>
    {
        send_mail(self, envelop, OnError::StopAndReset)
    }
}