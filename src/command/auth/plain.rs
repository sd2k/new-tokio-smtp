use std::rc::Rc;
use std::sync::Arc;

use bytes::BufMut;
use futures::future::Future;
use base64::encode;

use ::{Connection, CmdFuture, Cmd, Io, EhloData};
use ::error::MissingCapabilities;
use ::io::CR_LF;
use super::validate_auth_capability;

/// AUTH PLAIN smtp authentification based on rfc4954/rfc4616
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
                               &*self.authorization_identity,
                               &self.authentication_identity,
                               &self.password));

        const CMD_BASE: &str = "AUTH PLAIN ";

        let mut io = con.into_inner();
        let len_needed = CMD_BASE.len() + auth_str.len() + CR_LF.len();
        {
            let buf = io.out_buffer(len_needed);
            buf.put(CMD_BASE);
            buf.put(auth_str);
            buf.put(CR_LF);
        }

        let fut = io.flush()
            .and_then(Io::parse_response)
            .map(|(io, res)| (Connection::from(io), res));

        Box::new(fut)
    }
}

impl Cmd for AuthPlain {

    fn check_cmd_avilability(&self, caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        validate_auth_capability(caps, "PLAIN")
    }

    fn exec(self, con: Connection) -> CmdFuture {
        self.exec_ref(con)
    }
}

impl Cmd for Rc<AuthPlain> {

    fn check_cmd_avilability(&self, caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        let me: &AuthPlain = &*self;
        me.check_cmd_avilability(caps)
    }

    fn exec(self, con: Connection) -> CmdFuture {
        self.exec_ref(con)
    }
}

impl Cmd for Arc<AuthPlain> {

    fn check_cmd_avilability(&self, caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        let me: &AuthPlain = &*self;
        me.check_cmd_avilability(caps)
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

