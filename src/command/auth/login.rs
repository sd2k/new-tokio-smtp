use base64::encode;
use futures::future::{self, Either, Future};

use super::validate_auth_capability;
use crate::{
    error::{LogicError, MissingCapabilities},
    future_ext::ResultWithContextExt,
    Cmd, EhloData, ExecFuture, Io,
};

/// Simple implementation of AUTH LOGIN for smtp.
#[derive(Debug, Clone)]
pub struct Login {
    username: String,
    password: String,
}

impl Login {
    /// Create a new auth login command based on username and password.
    pub fn new(username: &str, password: &str) -> Self {
        Login {
            username: encode(username),
            password: encode(password),
        }
    }

    /// Create a new auth login command based on base64 encoded username and password.
    pub fn from_base64(username: String, password: String) -> Self {
        Login { username, password }
    }

    /// Returns the username contained in the `Login` command.
    pub fn base64_username(&self) -> &str {
        &self.username
    }

    //intentionally no base64_password!
}

impl Cmd for Login {
    fn check_cmd_availability(&self, caps: Option<&EhloData>) -> Result<(), MissingCapabilities> {
        validate_auth_capability(caps, "LOGIN")
    }

    fn exec(self, mut io: Io) -> ExecFuture {
        let Login { username, password } = self;

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
            });

        Box::new(fut)
    }
}
