use std::io as std_io;
use std::net::{SocketAddr, IpAddr, Ipv4Addr, Ipv6Addr};
use std::fmt::Debug;
use std::collections::HashMap;
use native_tls::{self, TlsConnectorBuilder, TlsConnector};

use ::ascii::IgnoreAsciiCaseStr;

use ::data_types::{Domain, AddressLiteral, EhloParam, Capability};

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

impl From<IpAddr> for ClientIdentity {
    fn from(saddr: IpAddr) -> Self {
        let adl = AddressLiteral::from(saddr);
        ClientIdentity::from(adl)
    }
}

impl From<Ipv4Addr> for ClientIdentity {
    fn from(saddr: Ipv4Addr) -> Self {
        let adl = AddressLiteral::from(saddr);
        ClientIdentity::from(adl)
    }
}

impl From<Ipv6Addr> for ClientIdentity {
    fn from(saddr: Ipv6Addr) -> Self {
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

#[derive(Debug, Clone)]
pub struct EhloData {
    domain: Domain,
    data: HashMap<Capability, Vec<EhloParam>>
}

impl EhloData {

    pub fn new(domain: Domain, data: HashMap<Capability, Vec<EhloParam>>) -> Self {
        EhloData { domain, data }
    }

    pub fn has_capability<A>(&self, cap: A) -> bool
        where A: AsRef<str>
    {
        self.data.contains_key(<&IgnoreAsciiCaseStr>::from(cap.as_ref()))
    }

    pub fn get_capability_params<A>(&self, cap: A) -> Option<&[EhloParam]>
        where A: AsRef<str>
    {
        self.data.get(<&IgnoreAsciiCaseStr>::from(cap.as_ref()))
            .map(|vec| &**vec)
    }

    pub fn capability_map(&self) -> &HashMap<Capability, Vec<EhloParam>> {
        &self.data
    }

    pub fn domain(&self) -> &Domain {
        &self.domain
    }

}


impl Into<(Domain, HashMap<Capability, Vec<EhloParam>>)> for EhloData {
    fn into(self) -> (Domain, HashMap<Capability, Vec<EhloParam>>) {
        let EhloData { domain, data } = self;
        (domain, data)
    }
}