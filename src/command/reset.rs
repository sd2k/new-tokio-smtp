use std::io as std_io;

use futures::Future;
use future_ext::ResultWithContextExt;

use ::error::LogicError;
use ::{Cmd, CmdFuture, Connection, Io};
use ::response::codes;


#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Reset;

impl Cmd for Reset {

    fn exec(self, con: Connection) -> CmdFuture {
        let (io, ehlo) = con.split();

        let fut = io
            .flush_line("RSET")
            .and_then(Io::parse_response)
            // server should not, ever, answer with anything but 250, we can be tolerant and
            // accept all non-error codes but on error codes we have no way to handle it
            .and_then(|(io, result)| match result {
                Ok(response) => {
                    if response.code().is_positive() {
                        Ok((io, Ok(response)))
                    } else {
                        let logic_err = LogicError::UnexpectedCode(response);
                        Err(std_io::Error::new(std_io::ErrorKind::Other, logic_err))
                    }
                },
                Err(logic_err) => {
                    Err(std_io::Error::new(std_io::ErrorKind::Other, logic_err))
                }
            })
            .map(move |(io, result)| (Connection::from((io, ehlo)), result));

        Box::new(fut)

    }
}

