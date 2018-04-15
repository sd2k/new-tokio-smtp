use bytes::BufMut;
use futures::future::{self, Either, Future};
use future_ext::ResultWithContextExt;
use base64::encode;

use ::{Connection, CmdFuture, Cmd, Io, EhloData};
use ::io::CR_LF;
use ::error::{LogicError, MissingCapabilities};
use super::validate_auth_capability;

#[derive(Debug, Clone)]
pub struct AuthLogin {
    username: String,
    password: String
}

impl AuthLogin {

    pub fn new(username: &str, password: &str) -> Self {
        AuthLogin {
            username: encode(username),
            password: encode(password),
        }
    }

    pub fn from_base64(username: String, password: String) -> Self {
        AuthLogin { username, password }
    }

    pub fn base64_username(&self) -> &str {
        &self.username
    }

    //intentionally no base64_password!

}


impl Cmd for AuthLogin {

    fn check_cmd_avilability(&self, caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        validate_auth_capability(caps, "LOGIN")
    }

    fn exec(self, con: Connection) -> CmdFuture {
        const CMD_BASE: &str = "AUTH LOGIN ";
        //1. send| AUTH LOGIN <base64name>
        //2. recv| 334 <msg_as_base64>
        //3. send| <base64password>
        //4. recv| 235 2.7.0 Accepted

        let mut io = con.into_inner();
        let AuthLogin { username, password } = self;

        let len_needed = CMD_BASE.len() + username.len() + CR_LF.len();
        {
            let buf = io.out_buffer(len_needed);
            buf.put(CMD_BASE);
            buf.put(username);
            buf.put(CR_LF);
        }

        let fut = io.flush()
            .and_then(Io::parse_response)
            .ctx_and_then(move |io: Io, response| {
                if !response.code().is_intermediate() {
                    Either::A(future::ok((io, Err(LogicError::UnexpectedCode(response)))))
                } else {
                    let fut = io
                        .flush_line(password.as_str())
                        .and_then(Io::parse_response);

                    Either::B(fut)
                }
            })
            .map(move |(io, res)| (Connection::from(io), res));

        Box::new(fut)

    }
}