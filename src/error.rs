use std::{io as std_io};
use std::error::Error;
use std::fmt::{self, Display, Debug};
use ::data_types::{Capability, EsmtpKeyword};
use ::response::Response;

#[derive(Debug)]
pub enum ConnectingFailed {
    Io(std_io::Error),
    Setup(LogicError),
    Auth(LogicError)
}

impl From<std_io::Error> for ConnectingFailed {
    fn from(err: std_io::Error) -> Self {
        ConnectingFailed::Io(err)
    }
}

impl Error for ConnectingFailed {
    fn description(&self) -> &str {
       "connecting with server failed"
    }

    fn cause(&self) -> Option<&Error> {
        use self::ConnectingFailed::*;
        match *self {
            Io(ref err) => Some(err),
            Setup(ref err) => Some(err),
            Auth(ref err) => Some(err)
        }
    }
}

impl Display for ConnectingFailed {
    fn fmt(&self, fter: &mut fmt::Formatter) -> fmt::Result {
        //TODO better implementation
        Debug::fmt(self, fter)
    }
}

pub fn check_response(response: Response) -> Result<Response, LogicError> {
    if response.is_erroneous() {
        Err(LogicError::Code(response))
    } else {
        Ok(response)
    }
}

#[derive(Debug)]
pub enum LogicError {
    /// The server replied with a error response code
    Code(Response),

    /// The server replied with a non-error response code, but the command could not handle it
    ///
    /// For example on DATA the server responds with the intermediate code 354, if the client
    /// now receives e.g. a 240 than clearly something went wrong.
    UnexpectedCode(Response),

    /// a custom error code
    ///
    /// This is meant to be produced by a custom command, as the sender of the command knows
    /// (at some abstraction level) which command it send, it can downcast and handle the
    /// error
    Custom(Box<Error + 'static + Send + Sync>),

    /// command can not be used, as the server does not promotes the necessary capabilities
    MissingCapabilities(MissingCapabilities)
}

impl From<MissingCapabilities> for LogicError {
    fn from(err: MissingCapabilities) -> Self {
        LogicError::MissingCapabilities(err)
    }
}



impl Error for LogicError {

    fn description(&self) -> &str {
        use self::LogicError::*;
        match *self {
            Code(_) => "server responded with error response code",
            UnexpectedCode(_) => "server responded with unexpected non-error response code",
            MissingCapabilities(ref err) => err.description(),
            Custom(ref boxed) => boxed.description()
        }
    }

    fn cause(&self) -> Option<&Error> {
        use self::LogicError::*;
        match *self {
            Custom(ref boxed) => boxed.cause(),
            _ => None
        }
    }
}

impl Display for LogicError {

    fn fmt(&self, fter: &mut fmt::Formatter) -> fmt::Result {
        use self::LogicError::*;

        match *self {
            Custom(ref boxed) => Display::fmt(boxed, fter),
            //FIXME better display impl
            _ => Debug::fmt(self, fter),
        }
    }
}


#[derive(Debug, Clone)]
pub struct MissingCapabilities {
    capabilities: Vec<Capability>
}

impl MissingCapabilities {

    pub fn new_from_str_unchecked<I>(data: I) -> Self
        where I: Into<String>
    {
        MissingCapabilities::new(vec![
            Capability::from(EsmtpKeyword::from_str_unchecked(data.into()))
        ])
    }

    pub fn new(capabilities: Vec<Capability>) -> Self {
        MissingCapabilities { capabilities }
    }

    pub fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }
}

impl Into<Vec<Capability>> for MissingCapabilities {
    fn into(self) -> Vec<Capability> {
        let MissingCapabilities { capabilities } = self;
        capabilities
    }
}

impl From<Vec<Capability>> for MissingCapabilities {
    fn from(capabilities: Vec<Capability>) -> Self {
        MissingCapabilities { capabilities }
    }
}

impl Error for MissingCapabilities {
    fn description(&self) -> &str {
        "missing capabilities to run command"
    }
}

impl Display for MissingCapabilities {

    fn fmt(&self, fter: &mut fmt::Formatter) -> fmt::Result {
        write!(fter, "missing capabilities:")?;
        let mut first = true;
        for cap in self.capabilities.iter() {
            let str_cap = cap.as_str();
            if first {
                write!(fter, " {}", str_cap)?;
            } else {
                write!(fter, ", {}", str_cap)?;
            }
            first = false;
        }
        Ok(())
    }
}