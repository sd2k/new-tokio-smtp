use futures::future::{self, Either, Future};
use base64::encode;

use ::future_ext::ResultWithContextExt;
use ::{Connection, CmdFuture, Cmd, Io, EhloData};
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

    fn check_cmd_availability(&self, caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        validate_auth_capability(caps, "LOGIN")
    }

    fn exec(self, con: Connection) -> CmdFuture {

        let mut io = con.into_inner();
        let AuthLogin { username, password } = self;

        io.write_line_from_parts(&["AUTH LOGIN", username.as_str()]);

        let fut = io
            .flush()
            .and_then(Io::parse_response)
            .ctx_and_then(move |io: Io, response| {
                if !response.code().is_intermediate() {
                    Either::A(future::ok((io, Err(LogicError::UnexpectedCode(response)))))
                } else {
                    let fut = io
                        .flush_line_from_parts(&[password.as_str()])
                        .and_then(Io::parse_response);

                    Either::B(fut)
                }
            })
            .map(move |(io, res)| (Connection::from(io), res));

        Box::new(fut)

    }
}