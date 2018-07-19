use std::io as std_io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::fmt::Debug;
use std::collections::HashMap;

use native_tls::{self, TlsConnectorBuilder, TlsConnector};
use hostname::get_hostname;


use ::ascii::IgnoreAsciiCaseStr;
use ::data_types::{Domain, AddressLiteral, EhloParam, Capability};

/// Represents the identity of an client
///
/// If you connect to an MSA this can be as simple as
/// localhost, through for smtp communication between
/// servers or for connecting with an MX server this
/// should be a public facing domain or ip address
///
/// ---
///
/// MSA: Mail Submission Agent
///
/// MX: Mail Exchanger
///
#[derive(Debug, Clone)]
pub enum ClientIdentity {
    /// a registered domain
    Domain(Domain),
    /// a ipv4/ipv6 address, through theoretically others protocols are
    /// possible too
    AddressLiteral(AddressLiteral)
}

impl ClientIdentity {

    /// creates a client identity for "localhost" (here fixed to 127.0.0.1)
    ///
    /// This can be used as client identity when connecting a mail client to
    /// a Mail Submission Agent (MSA), but should not be used when connecting
    /// to an Mail Exchanger (MX).
    pub fn localhost() -> Self {
        //TODO use "domain" localhost??
        Self::from(Ipv4Addr::new(127, 0, 0, 1))
    }

    /// creates a client identity using hostname (fallback localhost)
    ///
    /// This uses the `hostname` crate to create a client identity.
    /// If this fails `ClientIdentity::localhost()` is used.
    ///
    pub fn hostname() -> Self {
        Self::try_hostname()
            .unwrap_or_else(|| Self::localhost())
    }

    /// creates a client identity if a hostname can be found
    ///
    /// # Implementation Note
    ///
    /// As the `hostname` crate currently only returns an `Option`
    /// we also do so.
    pub fn try_hostname() -> Option<Self> {
        get_hostname()
            .map(|name| {
                //SEMANTIC_SAFE: the systems hostname should be a valid domain (syntactically)
                let domain = Domain::new_unchecked(name);
                ClientIdentity::Domain(domain)
            })
    }
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

/// A Tls configuration
///
/// This consists of a domain, which is the domain of the
/// server we connect to and a `SetupTls` instance,
/// which can be used to modify the tls setup e.g. to
/// use a client certificate for authentication.
///
/// The `SetupTls` default to `DefaultTlsSetup` which
/// is enough for most use cases.
#[derive(Debug, Clone)]
pub struct TlsConfig<S = DefaultTlsSetup>
    where S: SetupTls
{
    /// domain of the server we connect to
    pub domain: Domain,
    /// setup allowing modifying TLS setup process
    pub setup: S
}

impl From<Domain> for TlsConfig {
    fn from(domain: Domain) -> Self {
        TlsConfig { domain, setup: DefaultTlsSetup }
    }
}

/// Trait used when setting up tls to modify the setup process
pub trait SetupTls: Debug + Send + 'static {

    /// Accepts a connection builder and returns a connector if possible
    fn setup(self, builder: TlsConnectorBuilder) -> Result<TlsConnector, native_tls::Error>;
}

/// The default tls setup, which just calls `builder.build()`
#[derive(Debug, Clone)]
pub struct DefaultTlsSetup;

impl SetupTls for DefaultTlsSetup {
    fn setup(self, builder: TlsConnectorBuilder) -> Result<TlsConnector, native_tls::Error> {
        builder.build()
    }
}

impl<F: 'static> SetupTls for F
    where F: Send + Debug + FnOnce(TlsConnectorBuilder) -> Result<TlsConnector, native_tls::Error>
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

/// A type representing the ehlo response of the last ehlo call
///
/// This is mainly used to check if a certain capability/command
/// is supported. E.g. if SMTPUTF8 is supported.
#[derive(Debug, Clone)]
pub struct EhloData {
    domain: Domain,
    data: HashMap<Capability, Vec<EhloParam>>
}

impl EhloData {

    /// create a new Ehlo data from the domain with which the server responded and the
    /// ehlo parameters of the response
    pub fn new(domain: Domain, data: HashMap<Capability, Vec<EhloParam>>) -> Self {
        EhloData { domain, data }
    }

    /// check if a ehlo contained a specific capability e.g. `SMTPUTF8`
    pub fn has_capability<A>(&self, cap: A) -> bool
        where A: AsRef<str>
    {
        self.data.contains_key(<&IgnoreAsciiCaseStr>::from(cap.as_ref()))
    }

    /// get the parameters for a specific capability e.g. the size of `SIZE`
    pub fn get_capability_params<A>(&self, cap: A) -> Option<&[EhloParam]>
        where A: AsRef<str>
    {
        self.data.get(<&IgnoreAsciiCaseStr>::from(cap.as_ref()))
            .map(|vec| &**vec)
    }

    /// return a reference to the inner hash map
    pub fn capability_map(&self) -> &HashMap<Capability, Vec<EhloParam>> {
        &self.data
    }

    /// the domain for which the server acts
    pub fn domain(&self) -> &Domain {
        &self.domain
    }

}

impl From<(Domain, HashMap<Capability, Vec<EhloParam>>)> for EhloData {
    fn from((domain, map): (Domain, HashMap<Capability, Vec<EhloParam>>)) -> Self {
        EhloData::new(domain, map)
    }
}

impl Into<(Domain, HashMap<Capability, Vec<EhloParam>>)> for EhloData {
    fn into(self) -> (Domain, HashMap<Capability, Vec<EhloParam>>) {
        let EhloData { domain, data } = self;
        (domain, data)
    }
}
