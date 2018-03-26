use std::{io as std_io};

use bytes::{BytesMut, BufMut, Buf};
use futures::future::{self, Either, Future};
use futures::stream::Stream;

use native_tls::{self, TlsConnector, TlsConnectorBuilder};
use tokio_tls::TlsConnectorExt;


// cyclic dep. for double dispatch ergonomics
use ::{Connection, CmdFuture, Cmd, SimpleCmd};
use ::io::{Io, Socket, Buffers};
use ::response::{Response, codes};




macro_rules! impl_simple_command {
    ($(for $name:ident => |$buf:ident| $block:block;)*) => ($(
        impl Cmd for $name {
            #[inline]
            fn exec(self, con: Connection) -> CmdFuture {
                con.simple_cmd(self)
            }
        }

        impl SimpleCmd for $name {
            fn write_cmd(&self, $buf: &mut BytesMut) {
                $block
            }
        }
    )*);
}



pub struct Reset;
pub struct Quit;

impl_simple_command! {
    for Reset => |buf| { buf.put("RSET") };
    for Quit => |buf| { buf.put("QUIT") };
}


pub trait SetupTls: 'static {
    fn setup(self, builder: TlsConnectorBuilder) -> Result<TlsConnector, native_tls::Error>;
}

impl<F: 'static> SetupTls for F
    where F: FnOnce(TlsConnectorBuilder) -> Result<TlsConnector, native_tls::Error>
{
    fn setup(self, builder: TlsConnectorBuilder) -> Result<TlsConnector, native_tls::Error> {
        (self)(builder)
    }
}

pub struct DefaultSetup;

impl SetupTls for DefaultSetup {
    fn setup(self, builder: TlsConnectorBuilder) -> Result<TlsConnector, native_tls::Error> {
        builder.build()
    }
}

pub struct StartTls<S = DefaultSetup> {
    setup_tls: S,
    sni_domain: String,
}

impl StartTls<DefaultSetup> {
    pub fn new<I>(sni_domain: I) -> Self
        where I: Into<String>
    {
        StartTls {
            sni_domain: sni_domain.into(),
            setup_tls: DefaultSetup
        }
    }
}

impl<S> StartTls<S>
    where S: SetupTls
{

    pub fn new_with_tls_setup<I, F: 'static>(sni_domain: I, setup_tls: S) -> Self
        where I: Into<String>
    {
        StartTls {
            setup_tls,
            sni_domain: sni_domain.into(),
        }
    }
}


//FIXME[rust/catch]: use catch once in stable
macro_rules! alttry {
    ($block:block => $emap:expr) => ({
        let func = move || -> Result<_, _> { $block };
        match func() {
            Ok(ok)  => ok,
            Err(err) => return ($emap)(err)
        }
    });
}

fn map_tls_err(err: native_tls::Error) -> std_io::Error {
    std_io::Error::new(
        std_io::ErrorKind::Other,
        err
    )
}

impl<S> Cmd for StartTls<S>
    where S: SetupTls
{

    fn exec(self, con: Connection) -> CmdFuture {
        let (io, ehlo_data) = con.destruct();
        let StartTls { sni_domain, setup_tls } = self;

        if io.is_secure() {
            let fut = future::err(std_io::Error::new(
                std_io::ErrorKind::AlreadyExists,
                "connection is already TLS encrypted"
            ));
            return Box::new(fut);
        }

        let fut = io
            .flush_cmd("STARTTLS")
            .and_then(Io::parse_response)
            .and_then(move |(io, smtp_result)| match smtp_result {
                Err(response) => {
                    let con = Connection::from((io, ehlo_data));
                    Either::A(future::ok((con, Err(response))))
                },
                Ok(_) => {
                    let connector = alttry!(
                        {
                            setup_tls.setup(TlsConnector::builder()?)
                        } =>
                        |err| Either::A(future::err(map_tls_err(err)))
                    );

                    let (socket, _buffer) = io.destruct();
                    let stream = match socket {
                        Socket::Insecure(stream) => stream,
                        _ => unreachable!()
                    };

                    let fut = connector
                        .connect_async(&sni_domain, stream)
                        .map_err(map_tls_err)
                        .map(move |stream| {
                            let socket = Socket::Secure(stream);
                            let io = Io::from((socket, Buffers::new()));
                            let con = Connection::from((io, None));
                            let response = Ok(Response::new(
                                codes::READY,
                                vec![ "ready".into() ]
                            ));
                            (con, response)
                        });

                    Either::B(fut)
                },
            });

        Box::new(fut)
    }
}


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