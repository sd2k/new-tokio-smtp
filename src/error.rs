use std::error::Error;
use ::response::Response;

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
    Custom(Box<Error + 'static>)
}

pub fn check_response(response: Response) -> Result<Response, LogicError> {
    if response.is_erroneous() {
        Err(LogicError::Code(response))
    } else {
        Ok(response)
    }
}