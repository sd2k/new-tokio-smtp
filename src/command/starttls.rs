use std::io as std_io;

use futures::future::{self, Either, Future};

use native_tls::TlsConnector as NativeTlsConnector;
use tokio_tls::TlsConnector;

use crate::{
    error::MissingCapabilities,
    io::{Io, Socket},
    map_tls_err,
    response::{codes, Response},
    Capability, Cmd, DefaultTlsSetup, Domain, EhloData, EsmtpKeyword, ExecFuture, SetupTls,
};

pub struct StartTls<S = DefaultTlsSetup> {
    pub setup_tls: S,
    pub sni_domain: Domain,
}

impl StartTls<DefaultTlsSetup> {
    pub fn new<I>(sni_domain: I) -> Self
    where
        I: Into<Domain>,
    {
        StartTls {
            sni_domain: sni_domain.into(),
            setup_tls: DefaultTlsSetup,
        }
    }
}

impl<S> StartTls<S>
where
    S: SetupTls,
{
    pub fn new_with_tls_setup<I, F: 'static>(sni_domain: I, setup_tls: S) -> Self
    where
        I: Into<Domain>,
    {
        StartTls {
            setup_tls,
            sni_domain: sni_domain.into(),
        }
    }
}

/// STARTTLS is the only command which does not have a "final" response,
/// after it's intermediate response it will start the tls handshake and
/// after that nothing is ever send back, but this API _always_ has a
/// response for a request, so we create a "fake" response (`"220 Ready"`)
fn tls_done_result() -> Response {
    Response::new(codes::STATUS_RESPONSE, vec!["Ready".to_owned()])
}

fn connection_already_secure_error_future() -> ExecFuture {
    let fut = future::err(std_io::Error::new(
        std_io::ErrorKind::AlreadyExists,
        "connection is already TLS encrypted",
    ));
    return Box::new(fut);
}

const STARTTLS: &str = "STARTTLS";

impl<S> Cmd for StartTls<S>
where
    S: SetupTls,
{
    fn check_cmd_availability(&self, caps: Option<&EhloData>) -> Result<(), MissingCapabilities> {
        caps.and_then(|ehlo_data| {
            if ehlo_data.has_capability(STARTTLS) {
                Some(())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            let mcap = Capability::from(EsmtpKeyword::from_unchecked(STARTTLS));
            MissingCapabilities::new(vec![mcap])
        })
    }

    fn exec(self, mut io: Io) -> ExecFuture {
        let StartTls {
            sni_domain,
            setup_tls,
        } = self;

        let was_mock = match *io.socket_mut() {
            Socket::Insecure(_) => false,
            Socket::Secure(_) => {
                return connection_already_secure_error_future();
            }
            #[cfg(feature = "mock-support")]
            Socket::Mock(ref mut socket_mock) => {
                if socket_mock.is_secure() {
                    return connection_already_secure_error_future();
                } else {
                    socket_mock.set_is_secure(true);
                    true
                }
            }
        };

        if was_mock {
            let fut = future::ok((io, Ok(tls_done_result())));
            return Box::new(fut);
        }

        let fut = io
            .flush_line_from_parts(&["STARTTLS"])
            .and_then(Io::parse_response)
            .and_then(move |(io, smtp_result)| match smtp_result {
                Err(response) => Either::A(future::ok((io, Err(response)))),
                Ok(_) => {
                    let connector = alttry!(
                        {
                            let contor = setup_tls.setup(NativeTlsConnector::builder())?;
                            Ok(TlsConnector::from(contor))
                        } =>
                        |err| Either::A(future::err(map_tls_err(err)))
                    );

                    let (socket, _buffer, _ehlo_data) = io.split();
                    let stream = match socket {
                        Socket::Insecure(stream) => stream,
                        _ => unreachable!(),
                    };

                    let fut = connector
                        .connect(sni_domain.as_str(), stream)
                        .map_err(map_tls_err)
                        .map(move |stream| {
                            let socket = Socket::Secure(stream);
                            let io = Io::from(socket);
                            (io, Ok(tls_done_result()))
                        });

                    Either::B(fut)
                }
            });

        Box::new(fut)
    }
}
