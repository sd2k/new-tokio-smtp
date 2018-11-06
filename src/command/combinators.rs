use ::{ExecFuture, Cmd, Io, EhloData};
use ::error::{MissingCapabilities};

/// An either of two commands
///
/// Useful for cases when we need either of two commands but wouldn't like to use [`BoxedCmd`].
///
/// For example, `ConnectionConfig<EitherCmd<command::Noop, command::auth::Plain>>` would implement `Clone` trait, but ConnectionConfig<BoxedCmd> would not. So we can use once created config for connection several times.
///
/// ```
/// extern crate new_tokio_smtp;
///
/// use new_tokio_smtp::{command::{auth, Noop, EitherCmd}, ConnectionConfig};
///
/// fn main() {
///     let address = "127.0.0.1:25".parse().unwrap();
///     let hostname = "smtp.example.com".parse().unwrap();
///     let username = "user@example.com";
///     let password = "top-secret";
///
///     let auth_command = match auth::Plain::from_username(username, password) {
///         Ok(plain_auth) => EitherCmd::B(plain_auth),
///         Err(_) => EitherCmd::A(Noop),
///     };
///
///     let config = ConnectionConfig::builder_with_addr(address, hostname)
///         .auth(auth_command)
///         .build();
///
///     {
///         let config = config.clone();
///         // ...connect and send emails
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub enum EitherCmd<A, B> {
    A(A),
    B(B),
}

impl<A, B> Cmd for EitherCmd<A, B>
where
    A: Cmd,
    B: Cmd,
{
    fn check_cmd_availability(&self, caps: Option<&EhloData>) -> Result<(), MissingCapabilities> {
        match self {
            EitherCmd::A(a) => a.check_cmd_availability(caps),
            EitherCmd::B(b) => b.check_cmd_availability(caps),
        }
    }
    fn exec(self, con: Io) -> ExecFuture {
        match self {
            EitherCmd::A(a) => a.exec(con),
            EitherCmd::B(b) => b.exec(con),
        }
    }
}

/// An alternative of two commands
///
/// Useful for cases when we need execute only one of two commands depending from availability.
///
/// For example, `ConnectionConfig<SelectCmd<command::auth::Plain, command::auth::Login>>` can help alternate two auth methods.
///
/// ```
/// extern crate new_tokio_smtp;
///
/// use new_tokio_smtp::{command::{auth, SelectCmd}, ConnectionConfig};
///
/// fn main() {
///     let address = "127.0.0.1:25".parse().unwrap();
///     let hostname = "smtp.example.com".parse().unwrap();
///     let username = "user@example.com";
///     let password = "top-secret";
///
///     let plain_auth = auth::Plain::from_username(username, password).unwrap();
///     let login_auth = auth::Login::new(username, password);
///
///     let config = ConnectionConfig::builder_with_addr(address, hostname)
///         .auth(SelectCmd(plain_auth, login_auth))
///         .build();
///     // ...connect and send emails
/// }
/// ```
#[derive(Debug, Clone)]
pub struct SelectCmd<A, B>(pub A, pub B);

impl<A, B> Cmd for SelectCmd<A, B>
where
    A: Cmd,
    B: Cmd,
{
    fn check_cmd_availability(&self, caps: Option<&EhloData>) -> Result<(), MissingCapabilities> {
        self.0
            .check_cmd_availability(caps)
            .or_else(|_| self.1.check_cmd_availability(caps))
    }
    fn exec(self, con: Io) -> ExecFuture {
        if self.0.check_cmd_availability(con.ehlo_data()).is_ok() {
            Box::new(self.0.exec(con))
        } else {
            Box::new(self.1.exec(con))
        }
    }
}
