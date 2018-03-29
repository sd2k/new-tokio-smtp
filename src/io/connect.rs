use std::{io as std_io};
use std::net::SocketAddr;

use futures::future::{self, Map, Future};
use tokio::net::{TcpStream, ConnectFuture};
use tokio_tls::TlsConnectorExt;
use native_tls::TlsConnector;

use ::common::{map_tls_err, SetupTls, TlsConfig};
use super::Io;


impl Io {

    //FIXME[rust/impl Trait]: use -> impl Future<Item=Io, Error=std_io::Error>
    pub fn connect_insecure(addr: &SocketAddr) -> Map<ConnectFuture, fn(TcpStream) -> Io> {
        let fut = TcpStream
            ::connect(addr)
            .map(Io::from as fn(TcpStream) -> Io);

        fut
    }

    //FIXME[rust/impl Trait]: use -> impl Future<Item=Io, Error=std_io::Error>
    pub fn connect_secure<S>(addr: &SocketAddr, config: TlsConfig<S>)
        -> Box<Future<Item=Io, Error=std_io::Error> + 'static>
        where S: SetupTls
    {
        let TlsConfig { domain, setup } = config;
        let connector = alttry!(
            {
                setup.setup(TlsConnector::builder()?)
            } =>
            |err| Box::new(future::err(map_tls_err(err)))
        );

        let fut = TcpStream
            ::connect(&addr)
            .and_then(move |stream| connector
                .connect_async(domain.as_str(), stream)
                .map_err(map_tls_err)
            )
            .map(Io::from);

        Box::new(fut)

    }

}

