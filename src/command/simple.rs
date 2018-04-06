use std::collections::HashMap;

use bytes::{BytesMut, BufMut};
use data_types::{ReversePath, ForwardPath, EsmtpKeyword, EsmtpValue};

use ::{Connection, CmdFuture, Cmd, SimpleCmd};
use ::io::CR_LF;

#[macro_export]
macro_rules! impl_simple_command {
    ($(for $name:ident => |&$self:ident, $buf:ident| $block:block;)*) => ($(
        impl Cmd for $name {
            #[inline]
            fn exec(self, con: Connection) -> CmdFuture {
                con.send_simple_cmd(self)
            }
        }

        impl SimpleCmd for $name {
            fn write_cmd(&$self, $buf: &mut BytesMut) {
                $block
            }
        }
    )*);
}


#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Quit;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Noop;

pub type Params = HashMap<EsmtpKeyword, Option<EsmtpValue>>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Mail {
    pub reverse_path: ReversePath,
    pub params: Params
}

impl Mail {

    pub fn new(reverse_path: ReversePath) -> Self {
        Mail { reverse_path, params: Params::new() }
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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Verify {
    pub query: String
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Help {
    pub topic: Option<String>
}

impl_simple_command! {

    for Quit => |&self, buf| { buf.put("QUIT") };

    for Mail => |&self, buf| {
        const CMD: &str = "MAIL FROM:";

        let reverse_path = self.reverse_path.as_str();
        let params = &self.params;

        handle_pathy_cmd(CMD, reverse_path, params, buf);
    };

    for Recipient => |&self, buf| {
        const CMD: &str = "RCPT TO:";

        let forward_path = self.forward_path.as_str();
        let params = &self.params;

        handle_pathy_cmd(CMD, forward_path, params, buf);
    };

    for Verify => |&self, buf| {
        const CMD: &str = "VRFY ";
        let len = CMD.len() + self.query.len() + CR_LF.len();

        if len > buf.remaining_mut() {
            buf.reserve(len);
        }

        buf.put(CMD);
        buf.put(self.query.as_str());
    };

    for Help => |&self, buf| {
        const CMD: &str = "HELP";
        let len = CMD.len() + self.topic.as_ref().map(|t| t.len() + 1).unwrap_or(0) + CR_LF.len();

        if len > buf.remaining_mut() {
            buf.reserve(len);
        }

        buf.put(CMD);
        if let Some(topic) = self.topic.as_ref() {
            buf.put(" ");
            buf.put(topic.as_str());
        }
    };

    for Noop => |&self, buf| { buf.put("NOOP") };
}

fn handle_pathy_cmd(cmd: &str, path: &str, params: &Params, buf: &mut BytesMut) {
    let len = cmd.len()
        + path_len(path)
        + params_len(params)
        + CR_LF.len();

    if len > buf.remaining_mut() {
        buf.reserve(len);
    }

    buf.put(cmd);
    put_path_into_buffer(path, buf);
    put_params_into_buffer(params, buf);
}

fn path_len(path: &str) -> usize {
    //MAGIC_NUM: 2 = "<".len() + ">".len()
    path.len() + 2
}

fn put_path_into_buffer<B>(path: &str, buf: &mut B)
    where B: BufMut
{
    buf.put("<");
    buf.put(path);
    buf.put(">");
}

fn params_len(params: &HashMap<EsmtpKeyword, Option<EsmtpValue>>) -> usize {
    params.iter()
        .map(|(k,v)| {
            //MAGIC_NUM: " ".len() + "=".len() in ( SP <kw> [=<val>] )*;
            1 + k.as_str().len() + v.as_ref().map(|v| 1 + v.as_str().len()).unwrap_or(0)
        })
        .fold(0, |a,b| a + b)
}

fn put_params_into_buffer(
    params: &HashMap<EsmtpKeyword, Option<EsmtpValue>>, buf: &mut BytesMut
) {
    for (k, v) in params {
        buf.put(" ");
        buf.put(k.as_str());
        if let Some(v) = v.as_ref() {
            buf.put("=");
            buf.put(v.as_str());
        }
    }
}