use std::{io as std_io};

use bytes::{Buf, IntoBuf};
use futures::future::{self, Either, Future};
use futures::stream::{self, Stream};
use future_ext::ResultWithContextExt;

use ::{Connection, CmdFuture, Cmd, Io, EhloData};
use ::response::codes;
use ::error::{LogicError, MissingCapabilities};


pub struct Data<S> {
    //TODO add parameter support
    source: S
}

impl<BF> Data<stream::Once<BF, std_io::Error>>
    where BF: Buf
{
    pub fn from_buf<B: IntoBuf<Buf=BF>>(buf: B) -> Self {
        Data::new(stream::once(Ok(buf.into_buf())))
    }
}

impl<S> Data<S>
    where S: Stream<Error=std_io::Error>, S::Item: Buf
{
    pub fn new(source: S) -> Self {
        Data { source }
    }
}

impl<S: 'static> Cmd for Data<S>
    where S: Stream<Error=std_io::Error> + Send, S::Item: Buf
{

    fn check_cmd_availability(&self, _caps: Option<&EhloData>)
        -> Result<(), MissingCapabilities>
    {
        Ok(())
    }

    fn exec(self, con: Connection) -> CmdFuture {
        let io = con.into_inner();
        let Data { source } = self;

        let fut = io
            .flush_line_from_parts(&["DATA"])
            .and_then(Io::parse_response)
            .ctx_and_then(move |io, response| {
                if response.code() != codes::START_MAIL_DATA {
                    //TODO differ in error between Fault/IoError/TlsError(potential fault?)
                    return Either::A(future::ok((io, Err(LogicError::UnexpectedCode(response)))));
                }

                let fut = io
                    .write_dot_stashed(source)
                    .and_then(Io::parse_response);

                Either::B(fut)
            })
            .map(move |(io, result)| (Connection::from(io), result));

        Box::new(fut)
    }

}