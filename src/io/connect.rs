use std::{io as std_io};
use std::net::SocketAddr;

use futures::future::{self, Map, Either, Future};
use tokio::net::tcp::{TcpStream, ConnectFuture};
use tokio_tls::TlsConnector;
use native_tls::TlsConnector as NativeTlsConnector;

use ::common::{map_tls_err, SetupTls, TlsConfig};
use super::Io;


impl Io {

    /// create a new Tcp only connection to the given address
    pub fn connect_insecure(addr: &SocketAddr) -> Map<ConnectFuture, fn(TcpStream) -> Io> {
        let fut = TcpStream
            ::connect(addr)
            .map(Io::from as fn(TcpStream) -> Io);

        fut
    }

    /// create a new Tcp-Tls connection to the given address using the given tls config
    pub fn connect_secure<S>(addr: &SocketAddr, config: TlsConfig<S>)
        -> impl Future<Item=Io, Error=std_io::Error> + Send
        where S: SetupTls
    {
        let TlsConfig { domain, setup } = config;
        let connector = alttry!(
            {
                let contor = setup.setup(NativeTlsConnector::builder())?;
                Ok(TlsConnector::from(contor))
            } =>
            |err| Either::B(future::err(map_tls_err(err)))
        );

        let fut = TcpStream
            ::connect(&addr)
            .and_then(move |stream| connector
                .connect(domain.as_str(), stream)
                .map_err(map_tls_err)
            )
            .map(Io::from);

        Either::A(fut)
    }

}

