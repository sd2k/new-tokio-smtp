use std::io as std_io;
use std::net::SocketAddr;
use std::fmt::Debug;
use native_tls::{self, TlsConnectorBuilder, TlsConnector};

use common::Domain;

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

pub(crate) fn map_tls_err(err: native_tls::Error) -> std_io::Error {
    std_io::Error::new(
        std_io::ErrorKind::Other,
        err
    )
}

pub trait SetupTls: Debug + 'static {
    fn setup(self, builder: TlsConnectorBuilder) -> Result<TlsConnector, native_tls::Error>;
}

#[derive(Debug, Clone)]
pub struct SetupTlsData<S>
    where S: SetupTls
{
    pub addr: SocketAddr,
    pub domain: Domain,
    pub setup: S
}
