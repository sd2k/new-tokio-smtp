use std::io as std_io;

use bytes::{Buf, IntoBuf};
use futures::{
    future::{self, Either, Future},
    stream::{self, Stream},
};

use crate::{
    error::{LogicError, MissingCapabilities},
    future_ext::ResultWithContextExt,
    response::codes,
    Cmd, EhloData, ExecFuture, Io,
};

pub struct Data<S> {
    //TODO add parameter support
    source: S,
}

impl<BF> Data<stream::Once<BF, std_io::Error>>
where
    BF: Buf,
{
    pub fn from_buf<B: IntoBuf<Buf = BF>>(buf: B) -> Self {
        Data::new(stream::once(Ok(buf.into_buf())))
    }
}

impl<S> Data<S>
where
    S: Stream<Error = std_io::Error>,
    S::Item: Buf,
{
    pub fn new(source: S) -> Self {
        Data { source }
    }
}

impl<S: 'static> Cmd for Data<S>
where
    S: Stream<Error = std_io::Error> + Send,
    S::Item: Buf,
{
    fn check_cmd_availability(&self, _caps: Option<&EhloData>) -> Result<(), MissingCapabilities> {
        Ok(())
    }

    fn exec(self, io: Io) -> ExecFuture {
        let Data { source } = self;

        let fut = io
            .flush_line_from_parts(&["DATA"])
            .and_then(Io::parse_response)
            .ctx_and_then(move |io, response| {
                if response.code() != codes::START_MAIL_DATA {
                    return Either::A(future::ok((io, Err(LogicError::UnexpectedCode(response)))));
                }

                let fut = io.write_dot_stashed(source).and_then(Io::parse_response);

                Either::B(fut)
            });

        Box::new(fut)
    }
}
