use std::{io as std_io};

use futures::future::{self, Either, Future};

use native_tls::TlsConnector;
use tokio_tls::TlsConnectorExt;

use ::data_types::Domain;
use ::common::{map_tls_err, SetupTls, DefaultTlsSetup};
use ::{Connection, CmdFuture, Cmd};
use ::io::{Io, Socket, Buffers};
use ::response::{Response, codes};




pub struct StartTls<S = DefaultTlsSetup> {
    pub setup_tls: S,
    pub sni_domain: Domain,
}

impl StartTls<DefaultTlsSetup> {
    pub fn new<I>(sni_domain: I) -> Self
        where I: Into<Domain>
    {
        StartTls {
            sni_domain: sni_domain.into(),
            setup_tls: DefaultTlsSetup
        }
    }
}

impl<S> StartTls<S>
    where S: SetupTls
{

    pub fn new_with_tls_setup<I, F: 'static>(sni_domain: I, setup_tls: S) -> Self
        where I: Into<Domain>
    {
        StartTls {
            setup_tls,
            sni_domain: sni_domain.into(),
        }
    }
}

/// STARTTLS is the only command which does not have a "final" response,
/// after it's intermediate response it will start the tls handchake and
/// after that nothing is ever send back, but this API _always_ has a
/// response for a request, so we create a "fake" response (`"220 Ready"`)
fn tls_done_result() -> Response {
    Response::new(
        codes::STATUS_RESPONSE,
        vec![ "Ready".to_owned() ]
    )
}


fn connection_already_secure_error_future() -> CmdFuture {
    let fut = future::err(std_io::Error::new(
        std_io::ErrorKind::AlreadyExists,
        "connection is already TLS encrypted"
    ));
    return Box::new(fut);
}

impl<S> Cmd for StartTls<S>
    where S: SetupTls
{

    fn exec(self, con: Connection) -> CmdFuture {
        let (mut io, ehlo_data) = con.split();
        let StartTls { sni_domain, setup_tls } = self;

        let was_mock =
            match *io.socket_mut() {
                Socket::Insecure(_) => {
                    false
                },
                #[cfg(feature="mock_support")]
                Socket::Mock(ref mut socket_mock) if !socket_mock.is_secure() => {
                    socket_mock.set_is_secure(true);
                    true
                }
                #[cfg(feature="mock_support")]
                Socket::Secure(_) | Socket::Mock(_) => {
                    return connection_already_secure_error_future();
                }
                #[cfg(not(feature="mock_support"))]
                Socket::Secure(_) => {
                    return connection_already_secure_error_future();
                },
            };

        if was_mock {
            let con = Connection::from((io, ehlo_data));
            let fut = future::ok((con, Ok(tls_done_result())));
            return Box::new(fut);
        }

        let fut = io
            .flush_line("STARTTLS")
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

                    let (socket, _buffer) = io.split();
                    let stream = match socket {
                        Socket::Insecure(stream) => stream,
                        _ => unreachable!()
                    };

                    let fut = connector
                        .connect_async(sni_domain.as_str(), stream)
                        .map_err(map_tls_err)
                        .map(move |stream| {
                            let socket = Socket::Secure(stream);
                            let io = Io::from((socket, Buffers::new()));
                            let con = Connection::from((io, None));
                            (con, Ok(tls_done_result()))
                        });

                    Either::B(fut)
                },
            });

        Box::new(fut)
    }
}