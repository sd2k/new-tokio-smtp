use std::error::Error as ErrorTrait;
use std::fmt::{self, Display};
use std::sync::Arc;

use base64::encode;

use crate::{
    error::MissingCapabilities,
    Cmd, EhloData, ExecFuture, Io,
};

use super::validate_auth_capability;

/// AUTH PLAIN smtp authentication based on rfc4954/rfc4616
#[derive(Debug, Clone)]
pub struct Plain {
    authorization_identity: String,
    authentication_identity: String,
    password: String,
}

impl Plain {
    /// Create a auth plain command from a given username and password.
    pub fn from_username<I1, I2>(user: I1, password: I2) -> Result<Self, NullCodePointError>
    where
        I1: Into<String> + AsRef<str>,
        I2: Into<String> + AsRef<str>,
    {
        validate_no_null_cps(&user)?;
        validate_no_null_cps(&password)?;

        let user = user.into();
        Ok(Plain {
            authentication_identity: user.clone(),
            authorization_identity: user,
            password: password.into(),
        })
    }

    /// Create a auth plain command from a authorization identity a authentication identity and a password.
    ///
    /// Most times authorization and authentication identities are the same (and happen to be
    /// the username) in which case `auth::Plain::from_username` can be used.
    pub fn new<I1, I2, I3>(
        authorization_identity: I1,
        authentication_identity: I2,
        password: I3,
    ) -> Result<Self, NullCodePointError>
    where
        I1: Into<String> + AsRef<str>,
        I2: Into<String> + AsRef<str>,
        I3: Into<String> + AsRef<str>,
    {
        validate_no_null_cps(&authorization_identity)?;
        validate_no_null_cps(&authentication_identity)?;
        validate_no_null_cps(&password)?;

        Ok(Plain {
            authentication_identity: authentication_identity.into(),
            authorization_identity: authorization_identity.into(),
            password: password.into(),
        })
    }

    /// Returns the authorization identity which will be used.
    pub fn authorization_identity(&self) -> &str {
        &self.authorization_identity
    }

    /// Returns the authentication identity which will be used.
    pub fn authentication_identity(&self) -> &str {
        &self.authentication_identity
    }

    //intentionally no fn password(&self)!

    fn exec_ref(&self, io: Io) -> ExecFuture {
        let auth_str = encode(&format!(
            "{}\0{}\0{}",
            &self.authorization_identity, &self.authentication_identity, &self.password
        ));

        io.exec_simple_cmd(&["AUTH PLAIN ", auth_str.as_str()])
    }
}

impl Cmd for Plain {
    fn check_cmd_availability(&self, caps: Option<&EhloData>) -> Result<(), MissingCapabilities> {
        validate_auth_capability(caps, "PLAIN")
    }

    fn exec(self, con: Io) -> ExecFuture {
        self.exec_ref(con)
    }
}

impl Cmd for Arc<Plain> {
    fn check_cmd_availability(&self, caps: Option<&EhloData>) -> Result<(), MissingCapabilities> {
        let me: &Plain = &*self;
        me.check_cmd_availability(caps)
    }

    fn exec(self, con: Io) -> ExecFuture {
        self.exec_ref(con)
    }
}

fn validate_no_null_cps<R>(inp: R) -> Result<(), NullCodePointError>
where
    R: AsRef<str>,
{
    for bch in inp.as_ref().bytes() {
        if bch == b'\0' {
            return Err(NullCodePointError);
        }
    }
    Ok(())
}

/// Error returned if by auth plain if identity or password contained a null code point.
#[derive(Copy, Clone, Debug)]
pub struct NullCodePointError;

impl Display for NullCodePointError {
    fn fmt(&self, fter: &mut fmt::Formatter) -> fmt::Result {
        write!(fter, "input (username/password) contained null byte")
    }
}

impl ErrorTrait for NullCodePointError {}
