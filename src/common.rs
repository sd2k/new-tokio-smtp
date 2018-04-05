use std::io as std_io;
use std::net::SocketAddr;
use std::fmt::Debug;
use native_tls::{self, TlsConnectorBuilder, TlsConnector};

use ::data_types::{Domain, AddressLiteral};

#[derive(Debug, Clone)]
pub enum ClientIdentity {
    Domain(Domain),
    AddressLiteral(AddressLiteral)
}

impl From<Domain> for ClientIdentity {
    fn from(dm: Domain) -> Self {
        ClientIdentity::Domain(dm)
    }
}

impl From<AddressLiteral> for ClientIdentity {
    fn from(adl: AddressLiteral) -> Self {
        ClientIdentity::AddressLiteral(adl)
    }
}

impl From<SocketAddr> for ClientIdentity {
    fn from(saddr: SocketAddr) -> Self {
        let adl = AddressLiteral::from(saddr);
        ClientIdentity::from(adl)
    }
}

#[derive(Debug, Clone)]
pub struct TlsConfig<S = DefaultTlsSetup>
    where S: SetupTls
{
    pub domain: Domain,
    pub setup: S
}

impl From<Domain> for TlsConfig {
    fn from(domain: Domain) -> Self {
        TlsConfig { domain, setup: DefaultTlsSetup }
    }
}

#[derive(Debug, Clone)]
pub enum Security<S>
    where S: SetupTls
{
    None,
    DirectTls(TlsConfig<S>),
    StartTls(TlsConfig<S>)
}

#[derive(Debug, Clone)]
pub struct ConnectionConfig<S = DefaultTlsSetup>
    where S: SetupTls
{
    pub addr: SocketAddr,
    pub security: Security<S>,
    pub client_id: ClientIdentity
}

impl ConnectionConfig<DefaultTlsSetup> {

    pub fn with_direct_tls(addr: SocketAddr, domain: Domain, clid: ClientIdentity) -> Self {
        ConnectionConfig {
            addr,
            security: Security::DirectTls(domain.into()),
            client_id: clid
        }
    }

    pub fn with_starttls(addr: SocketAddr, domain: Domain, clid: ClientIdentity) -> Self {
        ConnectionConfig {
            addr,
            security: Security::StartTls(domain.into()),
            client_id: clid
        }
    }
}



pub trait SetupTls: Debug + 'static {
    fn setup(self, builder: TlsConnectorBuilder) -> Result<TlsConnector, native_tls::Error>;
}

#[derive(Debug, Clone)]
pub struct DefaultTlsSetup;

impl SetupTls for DefaultTlsSetup {
    fn setup(self, builder: TlsConnectorBuilder) -> Result<TlsConnector, native_tls::Error> {
        builder.build()
    }
}

impl<F: 'static> SetupTls for F
    where F: Debug + FnOnce(TlsConnectorBuilder) -> Result<TlsConnector, native_tls::Error>
{
    fn setup(self, builder: TlsConnectorBuilder) -> Result<TlsConnector, native_tls::Error> {
        (self)(builder)
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

pub(crate) fn map_tls_err(err: native_tls::Error) -> std_io::Error {
    std_io::Error::new(
        std_io::ErrorKind::Other,
        err
    )
}