use std::{io as std_io, net::SocketAddr};

use futures::future::{self, Either, Future, Map};
use native_tls::TlsConnector as NativeTlsConnector;
use tokio::net::tcp::{ConnectFuture, TcpStream};
use tokio_tls::TlsConnector;

use super::Io;
use crate::common::{map_tls_err, SetupTls, TlsConfig};

impl Io {
    /// create a new Tcp only connection to the given address
    pub fn connect_insecure(addr: &SocketAddr) -> Map<ConnectFuture, fn(TcpStream) -> Io> {
        let fut = TcpStream::connect(addr).map(Io::from as fn(TcpStream) -> Io);

        fut
    }

    /// create a new Tcp-Tls connection to the given address using the given tls config
    pub fn connect_secure<S>(
        addr: &SocketAddr,
        config: TlsConfig<S>,
    ) -> impl Future<Item = Io, Error = std_io::Error> + Send
    where
        S: SetupTls,
    {
        let TlsConfig { domain, setup } = config;
        let connector = alttry!(
            {
                let contor = setup.setup(NativeTlsConnector::builder())?;
                Ok(TlsConnector::from(contor))
            } =>
            |err| Either::B(future::err(map_tls_err(err)))
        );

        let fut = TcpStream::connect(&addr)
            .and_then(move |stream| {
                connector
                    .connect(domain.as_str(), stream)
                    .map_err(map_tls_err)
            })
            .map(Io::from);

        Either::A(fut)
    }
}
