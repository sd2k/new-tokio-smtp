use std::net::{SocketAddr, ToSocketAddrs};
use std::{io as std_io};
use std::fmt::Debug;

use futures::future::{self, Future, Either};

use ::future_ext::ResultWithContextExt;
use ::error::{
    ConnectingFailed,
    LogicError
};
use ::data_types::Domain;
use ::common::{
    TlsConfig, SetupTls,
    ClientId, DefaultTlsSetup
};
use ::io::{Io, SmtpResult};
use ::connection::{
    Connection, Cmd
};
//NOTE: out-of-order (potential circular) dep, but ok in this case
use ::command::Noop;

/// A future resolving to an `Connection` instance
pub type ConnectingFuture = Box<Future<Item=Connection, Error=ConnectingFailed> + Send + 'static>;

pub const DEFAULT_SMTP_MSA_PORT: u16 = 587;
pub const DEFAULT_SMTP_MX_PORT: u16 = 25;

fn cmd_future2connecting_future<LE: 'static, E>(
    res: Result<(Connection, SmtpResult), E>,
    new_logic_err: LE
) -> impl Future<Item=Connection, Error=ConnectingFailed> + Send
    where LE: Send + FnOnce(LogicError) -> ConnectingFailed,
          E: Into<ConnectingFailed>
{
    let fut =
        match res {
            Err(err) => Either::A(future::err(err.into())),
            Ok((con, Ok(_resp))) => Either::A(future::ok(con.into())),
            Ok((con, Err(err))) => {
                Either::B(con.quit().then(|_| Err(new_logic_err(err))))
            }
        };

    fut
}


impl Connection {

    /// open a connection to an smtp server using given configuration
    pub fn connect<S, A>(config: ConnectionConfig<A, S>)
        -> impl Future<Item=Connection, Error=ConnectingFailed> + Send
        where S: SetupTls, A: Cmd + Send
    {
        let ConnectionConfig { addr, security, client_id, auth_cmd } = config;

        #[allow(deprecated)]
        let con_fut = match security {
            Security::None => {
                Either::B(Either::A(Connection::_connect_insecure(&addr, client_id)))
            },
            Security::DirectTls(tls_config) => {
                Either::B(Either::B(Connection::_connect_direct_tls(&addr, client_id, tls_config)))
            }
            Security::StartTls(tls_config) => {
                Either::A(Connection::_connect_starttls(&addr, client_id, tls_config))
            }
        };

        let fut = con_fut
            .and_then(|con| con
                .send(auth_cmd)
                .then(|res| cmd_future2connecting_future(res, ConnectingFailed::Auth))
            );

        fut
    }

    #[doc(hidden)]
    pub fn _connect_insecure_no_ehlo(addr: &SocketAddr)
        -> impl Future<Item=Connection, Error=ConnectingFailed> + Send
    {
        let fut = Io
            ::connect_insecure(addr)
            .and_then(Io::parse_response)
            .then(|res| {
                let res = res.map(|(io, res)| (Connection::from(io), res));
                cmd_future2connecting_future(res, ConnectingFailed::Setup)
            });

        fut
    }

    #[doc(hidden)]
    pub fn _connect_direct_tls_no_ehlo<S>(addr: &SocketAddr, config: TlsConfig<S>)
        -> impl Future<Item=Connection, Error=ConnectingFailed> + Send
        where S: SetupTls
    {
        let fut = Io
            ::connect_secure(addr, config)
            .and_then(Io::parse_response)
            .then(|res| {
                let res = res.map(|(io, res)| (Connection::from(io), res));
                cmd_future2connecting_future(res, ConnectingFailed::Setup)
            });

        fut
    }

    #[doc(hidden)]
    pub fn _connect_insecure(addr: &SocketAddr, clid: ClientId)
        -> impl Future<Item=Connection, Error=ConnectingFailed> + Send
    {
        //Note: this has a circular dependency between Connection <-> cmd Ehlo which
        // could be resolved using a ext. trait, but it's more ergonomic this way
        use command::Ehlo;
        let fut = Connection
            ::_connect_insecure_no_ehlo(addr)
            .and_then(|con| con
                .send(Ehlo::from(clid))
                .then(|res| cmd_future2connecting_future(res, ConnectingFailed::Setup))
            );


        fut
    }

    #[doc(hidden)]
    pub fn _connect_direct_tls<S>(
        addr: &SocketAddr,
        clid: ClientId,
        config: TlsConfig<S>,
    ) -> impl Future<Item=Connection, Error=ConnectingFailed> + Send
        where S: SetupTls
    {
        //Note: this has a circular dependency between Connection <-> cmd Ehlo which
        // could be resolved using a ext. trait, but it's more ergonomic this way
        use command::Ehlo;
        let fut = Connection
            ::_connect_direct_tls_no_ehlo(addr, config)
            .and_then(|con| con
                .send(Ehlo::from(clid))
                .then(|res| cmd_future2connecting_future(res, ConnectingFailed::Setup))
            );

        fut
    }

    #[doc(hidden)]
    pub fn _connect_starttls<S>(
        addr: &SocketAddr,
        clid: ClientId,
        config: TlsConfig<S>
    )
        -> impl Future<Item=Connection, Error=ConnectingFailed> + Send
        where S: SetupTls
    {
        //Note: this has a circular dependency between Connection <-> cmd StartTls/Ehlo which
        // could be resolved using a ext. trait, but it's more ergonomic this way
        use command::{StartTls, Ehlo};
        let TlsConfig { domain, setup } = config;

        let fut = Connection
            ::_connect_insecure(&addr, clid.clone())
            .and_then(|con| con
                .send(StartTls {
                    setup_tls: setup,
                    sni_domain: domain
                })
                .map_err(ConnectingFailed::Io)
            )
            .ctx_and_then(|con, _| con
                .send(Ehlo::from(clid))
                .map_err(ConnectingFailed::Io)
            )
            .then(|res| cmd_future2connecting_future(res, ConnectingFailed::Setup));

        fut
    }
}

/// configure what kind of security is used
#[derive(Debug, Clone, PartialEq)]
pub enum Security<S>
    where S: SetupTls
{
    /// use a plain non encrypted connection
    #[deprecated(
        since="0.0",
        note="it's strongly discourage to use unencrypted connections for private information/auth etc.")]
    None,
    /// directly connect with TCP-TLS to smtp server
    DirectTls(TlsConfig<S>),
    /// connect with just TCP and then start TLS with the STARTTLS command
    StartTls(TlsConfig<S>)
}

/// Configuration specifing how to setup an SMTP connection.
///
/// Use the `ConnectionBuilder` to crate it.
/// (Expect if you need a unencrypted connection, in which
///  case you have to crate it by hand. It's not recommended
///  to use unencrypted connections for mail).
///
/// # Example
///
/// ```
/// use new_tokio_smtp::{ConnectionBuilder, Domain};
/// use new_tokio_smtp::command::auth::Login;
///
/// // For connecting with auth Login using the defaults, i.e.:
/// // STARTTLS, port 587 and the ip gotten from resolving
/// // the passed in domain/host name as well as the hostname
/// // as client identity.
/// let host = "smtp.gmail.com".parse()
///     .expect("malformed domain/host name");
/// let config = ConnectionBuilder
///     ::new(host)
///     .expect("could not resolve host name")
///     .auth(Login::new("user", "password"))
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct ConnectionConfig<A, S = DefaultTlsSetup>
    where S: SetupTls, A: Cmd
{
    /// the address and port to connect to (i.e. the ones of the smtp server)
    pub addr: SocketAddr,
    /// a command used for authentication (use NOOP if you don't auth)
    pub auth_cmd: A,
    /// the kind of TLS mechanism used when setting up the connection
    pub security: Security<S>,
    /// the client identity, i.e. your "identity"
    ///
    /// This is relevant for the communication between smtp server, through
    /// for connecting to an MSA (e.g. thunderbird connecting to gmail)
    /// using localhost (`[127.0.0.1]`) is enough
    pub client_id: ClientId
}

//IMPROVE: potentially crate a type safe builder chain
// e.g. ConnectionBuilder
//      ::connect_with_tls(addr, domain)/::connect_with_starttls(addr, domain)
//      .identity(ClientId) / .identitfy_as_localhost()
//      .auth(cmd) / .build() //uses auth Nop
//      .build()
impl<A> ConnectionConfig<A, DefaultTlsSetup>
    where A: Cmd
{

    /// create a connection config using direct tls
    ///
    /// This uses the default tls setup. The passed
    /// in domain is the domain in the certificate
    /// of the server used to make sure you connected
    /// to the right server (e.g. `smtp.ethereal.email`)
    pub fn with_direct_tls(addr: SocketAddr, domain: Domain, clid: ClientId, auth_cmd: A) -> Self {
        ConnectionConfig {
            addr, auth_cmd,
            security: Security::DirectTls(domain.into()),
            client_id: clid
        }
    }

    /// create a connection config using starttls
    ///
    /// This uses the default tls setup. The passed
    /// in domain is the domain in the certificate
    /// of the server used to make sure you connected
    /// to the right server (e.g. `smtp.ethereal.email`)
    pub fn with_starttls(addr: SocketAddr, domain: Domain, clid: ClientId, auth_cmd: A) -> Self {
        ConnectionConfig {
            addr, auth_cmd,
            security: Security::StartTls(domain.into()),
            client_id: clid
        }
    }

}


#[derive(Debug)]
pub struct ConnectionBuilder<A, S = DefaultTlsSetup>
    where S: SetupTls, A: Cmd
{
    client_id: Option<ClientId>,
    addr: SocketAddr,
    domain: Domain,
    setup_tls: S,
    use_security: UseSecurity,
    auth_cmd: A
}

impl ConnectionBuilder<Noop, DefaultTlsSetup> {


    /// Create a new `ConnectionBuilder` based on a domain name/host name.
    ///
    /// The used port will be `DEFAULT_SMTP_MSA_PORT` i.e. 587.
    /// The used socket address will be generate from using std's `ToSocketAddrs`
    /// with the given host and default port (the first address returned by
    /// `to_socket_addrs` is used, if there is non an `std_io::Error` is generated).
    ///
    /// # Error
    ///
    /// `std::net::ToSocketAddrs` is used internally and can cause an
    /// io error, e.g. if it can not resolve an address for the given
    /// host name.
    pub fn new(host: Domain) -> Result<Self, std_io::Error> {
        Self::new_with_port(host, DEFAULT_SMTP_MSA_PORT)
    }

    /// Create a new `ConnectionBuilder` based on a domain name/host name and port.
    ///
    /// The used socket address will be generate from using std's `ToSocketAddr`
    /// with the given host and the given port.
    ///
    /// # Error
    ///
    /// `std::net::ToSocketAddrs` is used internally and can cause an
    /// io error, e.g. if it can not resolve an address for the given
    /// host name.
    pub fn new_with_port(host: Domain, port: u16) -> Result<Self, std_io::Error> {
        let addr = get_addr((host.as_str(), port))?;
        Ok(Self::new_with_addr(addr, host))
    }

    /// Crate a new `ConnectionBuilder` based on a ip address, port and domain name.
    ///
    /// The domain name is used for Server Name Identification (SNI) and
    /// Tls hostname verification (hostname of the server).
    pub fn new_with_addr(addr: SocketAddr, domain: Domain) -> Self {
        ConnectionBuilder {
            addr,
            domain,
            use_security: UseSecurity::StartTls,
            client_id: None,
            setup_tls: DefaultTlsSetup,
            auth_cmd: Noop
        }
    }

}

impl<A, S> ConnectionBuilder<A, S>
    where S: SetupTls, A: Cmd
{
    /// Use a different `TlsSetup` implementation.
    ///
    /// This can be used if an advanced Tls configuration is needed,
    /// e.g. if you want to:
    ///
    /// - use client certificate authentication
    /// - change the min/max protocol version
    /// - add a root certificate
    /// - disable sni
    /// - and some crazy stuff like disable hostname verification, or certificate verification
    ///
    pub fn use_tls_setup<S2: SetupTls>(self, setup: S2) -> ConnectionBuilder<A, S2> {
        let ConnectionBuilder {
            addr, domain, use_security,
            client_id, setup_tls:_, auth_cmd
        } = self;

        ConnectionBuilder {
            addr, domain, use_security,
            client_id, setup_tls: setup, auth_cmd
        }
    }

    /// Make the builder use `STARTTLS` security when building.
    pub fn use_start_tls(mut self) -> Self {
        self.use_security = UseSecurity::StartTls;
        self
    }

    /// Make the builder use direct tls security when building.
    ///
    /// This is sometimes known as "wrapped" mode, it used a
    /// Tcp/Tls channel for transport instead of a pure Tcp
    /// channel.
    ///
    /// This often requires a different port as port 587 is
    /// reserved for "normal" mail submission (using the
    /// STARTTLS command) by RFC 6409.
    ///
    /// While direct tls is conform with smtp itself (RFC 5321)
    /// part of RFC 6409 which further specifies how the smtp
    /// should be used when a user (i.e. a mail program) wants
    /// to submit a mail to an Mail Submission Agent (MSA).
    pub fn use_direct_tls(mut self) -> Self {
        self.use_security = UseSecurity::DirectTls;
        self
    }

    /// Set the command to use for authentication.
    ///
    /// If this function is not called `Noop` is used,
    /// i.e. no authentication is done.
    pub fn auth<NA: Cmd>(self, auth_cmd: NA) -> ConnectionBuilder<NA, S> {
        let ConnectionBuilder {
            addr, domain, use_security,
            client_id, setup_tls, auth_cmd:_
        } = self;

        ConnectionBuilder {
            addr, domain, use_security,
            client_id, setup_tls, auth_cmd: auth_cmd
        }
    }

    /// Set's the client identity to the given identity.
    ///
    /// (The default is to use `ClientId::hostname()`)
    pub fn client_id(mut self, id: ClientId) -> Self {
        self.client_id = Some(id);
        self
    }


    /// Creates a new connection config.
    ///
    /// If not specified differently, then
    ///
    /// - `ClientId::hostname()` is used as `ClientId`
    /// - `Noop` is used as authentication command, i.e. no auth is done
    /// - `StartTls` is used as security method
    /// - `DefaultTlsSetup` is used for setting up tls (i.e. no special options are set)
    ///
    pub fn build(self) -> ConnectionConfig<A, S> {
        let ConnectionBuilder {
            addr, domain, use_security,
            client_id, setup_tls: setup, auth_cmd
        } = self;

        let tls_config = TlsConfig { domain, setup };
        let security =
            match use_security {
                UseSecurity::StartTls => Security::StartTls(tls_config),
                UseSecurity::DirectTls => Security::DirectTls(tls_config)
            };

        let client_id = client_id.unwrap_or_else(|| ClientId::hostname());

        ConnectionConfig {
            addr, security, auth_cmd, client_id
        }
    }
}


#[derive(Debug)]
enum UseSecurity {
    StartTls, DirectTls
}

fn get_addr(tsas: impl ToSocketAddrs + Copy + Debug) -> Result<SocketAddr, std_io::Error> {
    if let Some(addr) = tsas.to_socket_addrs()?.next() {
        Ok(addr)
    } else {
        Err(std_io::Error::new(std_io::ErrorKind::AddrNotAvailable,
            format!("{:?} is not associated with any socket address", tsas)))
    }
}

#[cfg(test)]
mod testd {
    use hostname::get_hostname;
    use super::*;

    //this domain has to exist
    const EXAMPLE_DOMAIN: &str = "1aim.com";

    #[test]
    fn builder_uses_right_defaults() {
        //
        let host = Domain::new_unchecked(EXAMPLE_DOMAIN.to_owned());
        let cb = ConnectionBuilder::new(host.clone()).unwrap();

        let ConnectionConfig {
            addr, security, auth_cmd, client_id
        } = cb.build();

        assert!(
            (EXAMPLE_DOMAIN, DEFAULT_SMTP_MSA_PORT)
            .to_socket_addrs()
            .unwrap()
            .any(|other_addr| other_addr == addr)
        );
        assert_eq!(security, Security::StartTls(TlsConfig {
            domain: host,
            setup: DefaultTlsSetup
        }));
        let _type_check: Noop = auth_cmd;
        if let ClientId::Domain(domain) = client_id {
            let expected_client_id = get_hostname()
                .unwrap_or_else(|| "localhost".to_owned());
            assert_eq!(domain.as_str(), &expected_client_id)
        } else {
            panic!("unexpected client id: {:?}", client_id);
        }
    }
}