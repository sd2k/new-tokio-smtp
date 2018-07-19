use std::fmt::{self, Display};
use std::sync::Arc;
use std::error::{Error as ErrorTrait};

use base64::encode;

use ::{ExecFuture, Cmd, EhloData, Io};
use ::error::MissingCapabilities;

use super::validate_auth_capability;

/// AUTH PLAIN smtp authentication based on rfc4954/rfc4616
#[derive(Debug, Clone)]
pub struct Plain {
    authorization_identity: String,
    authentication_identity: String,
    password: String
}

impl Plain {

    pub fn from_username<I1, I2>(user: I1, password: I2) -> Result<Self, NullCodePoint>
        where I1: Into<String> + AsRef<str>, I2: Into<String> + AsRef<str>
    {
        validate_no_null_cps(&user)?;
        validate_no_null_cps(&password)?;

        let user = user.into();
        Ok(Plain {
            authentication_identity: user.clone(),
            authorization_identity: user,
            password: password.into()
        })
    }

    pub fn new<I1,I2,I3>(
        authorization_identity: I1,
        authentication_identity: I2,
        password: I3
    ) -> Result<Self, NullCodePoint>
        where I1: Into<String> + AsRef<str>,
              I2: Into<String> + AsRef<str>,
              I3: Into<String> + AsRef<str>
    {
        validate_no_null_cps(&authorization_identity)?;
        validate_no_null_cps(&authentication_identity)?;
        validate_no_null_cps(&password)?;

        Ok(Plain {
            authentication_identity: authentication_identity.into(),
            authorization_identity: authorization_identity.into(),
            password: password.into()
        })
    }

    pub fn authorization_identity(&self) -> &str {
        &self.authorization_identity
    }

    pub fn authentication_identity(&self) -> &str {
        &self.authentication_identity
    }

    //intentionally no fn password(&self)!

    fn exec_ref(&self, io: Io) -> ExecFuture {
        let auth_str = encode(&format!("{}\0{}\0{}",
                               &self.authorization_identity,
                               &self.authentication_identity,
                               &self.password));

        io.exec_simple_cmd(&["AUTH PLAIN ", auth_str.as_str()])
    }
}

impl Cmd for Plain {

    fn check_cmd_availability(&self, caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        validate_auth_capability(caps, "PLAIN")
    }

    fn exec(self, con: Io) -> ExecFuture {
        self.exec_ref(con)
    }
}

impl Cmd for Arc<Plain> {

    fn check_cmd_availability(&self, caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        let me: &Plain = &*self;
        me.check_cmd_availability(caps)
    }

    fn exec(self, con: Io) -> ExecFuture {
        self.exec_ref(con)
    }
}

fn validate_no_null_cps<R>(inp: R) -> Result<(), NullCodePoint>
    where R: AsRef<str>
{
    for bch in inp.as_ref().bytes() {
        if bch == b'\0' {
            return Err(NullCodePoint)
        }
    }
    Ok(())
}

#[derive(Copy, Clone, Debug)]
pub struct NullCodePoint;

impl Display for NullCodePoint {
    fn fmt(&self, fter: &mut fmt::Formatter) -> fmt::Result {
        fter.write_str(self.description())
    }
}

impl ErrorTrait for NullCodePoint {
    fn description(&self) -> &str {
        "input (username/password) contained null byte"
    }

    fn cause(&self) -> Option<&ErrorTrait> {
        None
    }
}
