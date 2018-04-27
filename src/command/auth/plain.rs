use std::rc::Rc;
use std::sync::Arc;

use base64::encode;

use ::{Connection, CmdFuture, Cmd, EhloData};
use ::error::MissingCapabilities;

use super::validate_auth_capability;

/// AUTH PLAIN smtp authentication based on rfc4954/rfc4616
#[derive(Debug, Clone)]
pub struct AuthPlain {
    authorization_identity: String,
    authentication_identity: String,
    password: String
}

impl AuthPlain {

    //TODO check non null
    pub fn from_username<I1, I2>(user: I1, password: I2) -> Result<Self, NullCodePoint>
        where I1: Into<String> + AsRef<str>, I2: Into<String> + AsRef<str>
    {
        validate_no_null_cps(&user)?;
        validate_no_null_cps(&password)?;

        let user = user.into();
        Ok(AuthPlain {
            authentication_identity: user.clone(),
            authorization_identity: user,
            password: password.into()
        })
    }

    //TODO check non null
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

        Ok(AuthPlain {
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

    fn exec_ref(&self, con: Connection) -> CmdFuture {
        let auth_str = encode(&format!("{}\0{}\0{}",
                               &self.authorization_identity,
                               &self.authentication_identity,
                               &self.password));

        con.send_simple_cmd(&["AUTH PLAIN ", auth_str.as_str()])
    }
}

impl Cmd for AuthPlain {

    fn check_cmd_availability(&self, caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        validate_auth_capability(caps, "PLAIN")
    }

    fn exec(self, con: Connection) -> CmdFuture {
        self.exec_ref(con)
    }
}

impl Cmd for Rc<AuthPlain> {

    fn check_cmd_availability(&self, caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        let me: &AuthPlain = &*self;
        me.check_cmd_availability(caps)
    }

    fn exec(self, con: Connection) -> CmdFuture {
        self.exec_ref(con)
    }
}

impl Cmd for Arc<AuthPlain> {

    fn check_cmd_availability(&self, caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        let me: &AuthPlain = &*self;
        me.check_cmd_availability(caps)
    }

    fn exec(self, con: Connection) -> CmdFuture {
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

//TODO impl error
#[derive(Debug, Clone)]
pub struct NullCodePoint;

