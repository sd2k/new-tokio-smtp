use bytes::{BytesMut, BufMut};

// cyclic dep. for double dispatch ergonomics
use ::{Connection, CmdFuture, Cmd, SimpleCmd};

const CR_LF: &str = "\r\n";

#[macro_export]
macro_rules! impl_simple_command {
    ($(for $name:ident => |&$self:ident, $buf:ident| $block:block;)*) => ($(
        impl Cmd for $name {
            #[inline]
            fn exec(self, con: Connection) -> CmdFuture {
                con.simple_cmd(self)
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
pub struct Reset;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Quit;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Noop;

type ForwardPath = &'static str;
type ReversePath = &'static str;
type Param = &'static str;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Mail {
    reverse_path: ReversePath,
    params: Vec<Param>
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Recipient {
    //Grammar: "<Postmaster@" Domain ">" / "<Postmaster>" / forward-path
    //Note: that Postmaster is case-sensitive
    forward_path: ForwardPath,
    params: Vec<Param>
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Verify {
    query: String
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Help {
    topic: Option<String>
}

impl_simple_command! {
    for Reset => |&self, buf| { buf.put("RSET") };
    for Quit => |&self, buf| { buf.put("QUIT") };
    for Mail => |&self, buf| {
        const CMD: &str = "MAIL FROM:";

        let len = CMD.len()
            + self.reverse_path.len()
            + self.params.iter().map(|p| p.len()).fold(0, |s,v| s + v)
            + CR_LF.len();

        if len > buf.remaining_mut() {
            buf.reserve(len);
        }

        buf.put(CMD);
        buf.put(self.reverse_path);
        for param in &self.params {
            buf.put(param)
        }
    };
    for Recipient => |&self, buf| {
        const CMD: &str = "RCPT TO:";
        //MAGIC_NUM: 2 = "\r\n".len();
        let len = CMD.len()
            + self.forward_path.len()
            + self.params.iter().map(|p| p.len()).fold(0, |s,v| s + v)
            + CR_LF.len();

        if len > buf.remaining_mut() {
            buf.reserve(len);
        }

        buf.put(CMD);
        buf.put(self.forward_path);
        for param in &self.params {
            buf.put(param)
        }
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
