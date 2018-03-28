use std::{io as std_io};

use bytes::Buf;
use futures::future::{self, Either, Future};
use futures::stream::Stream;



// cyclic dep. for double dispatch ergonomics
use ::{Connection, CmdFuture, Cmd, Io};
use ::response::codes;


pub struct Data<S> {
    //TODO add parameter support
    source: S
}

impl<S: 'static> Cmd for Data<S>
    where S: Stream<Error=std_io::Error>, S::Item: Buf
{

    fn exec(self, con: Connection) -> CmdFuture {
        let (io, ehlo) = con.destruct();
        let Data { source } = self;

        let fut = io
            .flush_cmd("DATA")
            .and_then(Io::parse_response)
            .and_then(move |(io, result)| match result {
                Err(response) => {
                    let con = Connection::from((io, ehlo));
                    Either::A(future::ok((con, Err(response))))
                },
                Ok(response) => {
                    if response.code() != codes::START_MAIL_DATA {
                        //TODO differ in error between Fault/IoError/TlsError(potential fault?)
                        return Either::A(future::err(std_io::Error::new(
                            std_io::ErrorKind::Other,
                            "unexpected server response"
                        )));
                    }

                    let fut = io
                        .write_dot_stashed(source)
                        .and_then(Io::parse_response)
                        .map(|(io, result)| {
                            let con = Connection::from((io, ehlo));
                            (con, result)
                        });

                    Either::B(fut)
                }
            });

        Box::new(fut)
    }

}