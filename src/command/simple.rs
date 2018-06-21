use std::collections::HashMap;

use ::data_types::{ReversePath, ForwardPath, EsmtpKeyword, EsmtpValue};
use ::common::EhloData;
use ::error::MissingCapabilities;
use ::{ExecFuture, Cmd, Io};

/// Quit command, but as it makes the connection unusable we do
/// not publicly provide it for usage with `Connection::send`,
/// instead using `Connection::quit` is recommended.
#[doc(hidden)]
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Quit;

impl Cmd for Quit {

    fn check_cmd_availability(&self, _caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        Ok(())
    }

    fn exec(self, io: Io) -> ExecFuture {
        io.exec_simple_cmd(&["QUIT"])
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Noop;

impl Cmd for Noop {

    fn check_cmd_availability(&self, _caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        Ok(())
    }

    fn exec(self, io: Io) -> ExecFuture {
        io.exec_simple_cmd(&["NOOP"])
    }
}


pub type Params = HashMap<EsmtpKeyword, Option<EsmtpValue>>;

pub fn params_with_smtputf8(mut p: Params) -> Params {
    p.insert(EsmtpKeyword::from_unchecked("SMTPUTF8"), None);
    p
}

#[derive(Debug, Clone)]
pub struct Mail {
    pub reverse_path: ReversePath,
    pub params: Params
}

impl Mail {

    pub fn new(reverse_path: ReversePath) -> Self {
        Mail { reverse_path, params: Params::new() }
    }
}

impl Cmd for Mail {

    fn check_cmd_availability(&self, _caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        Ok(())
    }

    fn exec(self, con: Io) -> ExecFuture {
        handle_pathy_cmd(con, "MAIL FROM:", self.reverse_path.as_str(), &self.params)
    }
}


#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Recipient {
    //Grammar: "<Postmaster@" Domain ">" / "<Postmaster>" / forward-path
    //Note: that Postmaster is case-sensitive
    pub forward_path: ForwardPath,
    pub params: Params
}

impl Recipient {

    pub fn new(forward_path: ForwardPath) -> Self {
        Recipient { forward_path, params: Params::new() }
    }
}

impl Cmd for Recipient {

    fn check_cmd_availability(&self, _caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        Ok(())
    }

    fn exec(self, con: Io) -> ExecFuture {
        handle_pathy_cmd(con, "RCPT TO:", self.forward_path.as_str(), &self.params)
    }
}

fn handle_pathy_cmd(io: Io, cmd: &str, path: &str, params: &Params) -> ExecFuture {
    //no additional heap alloc
    if params.is_empty() {
        io.exec_simple_cmd(&[cmd, "<", path, ">"])
    } else {
        let mut parts = vec![cmd, "<", path, ">" ];
        for (k, v) in params.iter() {
            parts.push(" ");
            parts.push(k.as_str());
            if let Some(v) = v.as_ref() {
                parts.push("=");
                parts.push(v.as_str());
            }
        }
        io.exec_simple_cmd(parts.as_slice())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Verify {
    pub query: String
}

impl Cmd for Verify {
    fn check_cmd_availability(&self, _caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        Ok(())
    }

    fn exec(self, io: Io) -> ExecFuture {
        io.exec_simple_cmd(&["VRFY ", self.query.as_str()])
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Help {
    pub topic: Option<String>
}

impl Cmd for Help {
    fn check_cmd_availability(&self, _caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        Ok(())
    }

    fn exec(self, io: Io) -> ExecFuture {
        if let Some(topic) = self.topic.as_ref() {
            io.exec_simple_cmd(&["HELP ", topic.as_str()])
        } else {
            io.exec_simple_cmd(&["HELP"])
        }
    }
}

