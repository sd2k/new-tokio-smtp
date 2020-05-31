new-tokio-smtp [![docs](https://docs.rs/new-tokio-smtp/badge.svg)](https://docs.rs/new-tokio-smtp) [![License](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT) [![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
=====================

Maintenance Status
------------------

This crate is currently passively maintained, this means:

- I will still respond to bugs, and fix them (if this doesn't involve any *major* rewrites).
- I will still evaluate and merge PR's. (As long as I don't get flooded and they don't rewrite
  the whole crate or similar ;-)

Also the maintenance status might go back to actively maintained in the future.


Description
-------------

The new-tokio-smtp crate provides an extendible SMTP (Simple Mail Transfer Protocol)
implementation using tokio.

This crate provides _only_ SMTP functionality, this means it does neither
provides functionality for creating mails, nor for e.g. retrying sending
a mail if the receiver was temporary not available.

While it only provides SMTP functionality it is written in a way to
make it easy to integrate with higher level libraries. The interoperability
is provided through two mechanisms:

1. SMTP commands are defined in a way which allow library user to
   define there own commands, all commands provided by this library
   could theoretically have been implemented in an external library,
   this includes some of the more special commands like `STARTTLS`,
   `EHLO` and `DATA`. Moreover a `Connection` can be converted into
   a `Io` instance which provides a number of useful functionalities
   for easily implementing new commands, e.g. `Io.parse_response`.

2. syntactic construct's like e.g. `Domain` or `ClientId` can
   be parsed but also have "unchecked" constructors, this allows libraries
   which have there own validation to skip redundant validations, e.g.
   if a mail library might provide a `Mailbox` type of mail addresses and
   names, which is guaranteed to be syntactically correct if can implement
   a simple `From`/`Into` impl to cheaply convert it to an `Forward-Path`.
   (Alternative they also could implement their own `Mail` cmd if this
   has any benefit for them)

3. provided commands (and syntax constructs) are written in a robust way,
   allowing for example extensions like `SMTPUTF8` to be implemented on it.
   The only drawback of this is that it trusts that parts created by more
   higher level libraries are valid, e.g. it won't validate that the mail
   given to it is actually 7bit ascii or that it does not contain "orphan"
   `'\n'` (or `'\r'`) chars. But this is fine as this library is for using
   smtp to send mails, but _not_ for creating such mails. (Note that while
   it is trusting it does validate if a command can be used through checking
   the result from the last `EHLO` command, i.e. it wont allow you to send
   a `STARTTLS` command on a mail server not supporting it)

4. handling logic errors (i.e. server responded with code 550) separately
   from more fatal errors like e.g. a broken pipe

Example
---------

```rust
extern crate futures;
extern crate tokio;
extern crate new_tokio_smtp;
#[macro_use]
extern crate vec1;
extern crate rpassword;

use std::io::{stdin, stdout, Write};

use futures::stream::Stream;
use futures::future::lazy;
use new_tokio_smtp::error::GeneralError;
use new_tokio_smtp::{command, Connection, ConnectionConfig, Domain};
use new_tokio_smtp::send_mail::{
    Mail, EncodingRequirement,
    MailAddress, MailEnvelop,
};

struct Request {
    config: ConnectionConfig<command::auth::Plain>,
    mails: Vec<MailEnvelop>
}

fn main() {
    let Request { config, mails } = read_request();
    // We only have iter map overhead because we
    // don't have a failable mail encoding step, which normally is required.
    let mails = mails.into_iter().map(|m| -> Result<_, GeneralError> { Ok(m) });

    println!("[now starting tokio]");
    tokio::run(lazy(move || {
        println!("[start connect_send_quit]");
        Connection::connect_send_quit(config, mails)
            //Stream::for_each is design wise broken in futures v0.1
            .then(|result| Ok(result))
            .for_each(|result| {
                if let Err(err) = result {
                    println!("[sending mail failed]: {}", err);
                } else {
                    println!("[successfully send mail]")
                }
                Ok(())
            })
    }))
}


fn read_request() -> Request {

    println!("preparing to send mail with ethereal.email");
    let sender = read_email();
    let passwd = read_password();

    // The `from_unchecked` will turn into a `.parse()` in the future.
    let config = ConnectionConfig
        ::builder(Domain::from_unchecked("smtp.ethereal.email"))
            .expect("resolving domain failed")
        .auth(command::auth::Plain::from_username(sender.clone(), passwd)
            .expect("username/password can not contain \\0 bytes"))
        .build();

    // the from_unchecked normally can be used if we know the address is valid
    // a mail address parser will be added at some point in the future
    let send_to = MailAddress::from_unchecked("invalid@test.test");

    // using string fmt to crate mails IS A
    // REALLY BAD IDEA there are a ton of ways
    // this can go wrong, so don't do this in
    // practice, use some library to crate mails
    let raw_mail = format!(concat!(
        "Date: Thu, 14 Jun 2018 11:22:18 +0000\r\n",
        "From: You <{}>\r\n",
        //ethereal doesn't delivers any mail so it's fine
        "To: Invalid <{}>\r\n",
        "Subject: I am spam?\r\n",
        "\r\n",
        "...\r\n"
    ), sender.as_str(), send_to.as_str());

    // this normally adapts to a higher level abstraction
    // of mail then this crate provides
    let mail_data = Mail::new(EncodingRequirement::None, raw_mail.to_owned());

    let mail = MailEnvelop::new(sender, vec1![ send_to ], mail_data);

    Request {
        config,
        mails: vec![ mail ]
    }
}

fn read_email() -> MailAddress {
    let stdout = stdout();
    let mut handle = stdout.lock();
    write!(handle, "enter ethereal.email mail address\n[Note mail is not validated in this example]: ")
        .unwrap();
    handle.flush().unwrap();

    let mut line = String::new();
    stdin().read_line(&mut line).unwrap();
    MailAddress::from_unchecked(line.trim())
}

fn read_password() -> String {
    rpassword::prompt_password_stdout("password: ").unwrap()
}
```

Concept
--------

The concept of behind the library is explained
in the [notes/concept.md](./notes/concept.md) file.

Usability Helpers
------------------

The library provides a number of usability helpers:

1. `chain::chain` provides a easy way to chain a number of
    SMTP commands, sending each command when the previous
    command in the chain did not fail in any way.

2. `mock_support` feature:
    Extends the Socket abstraction to not only abstract over
    the socket being either a `TcpStream` or a `TlsStream` but
    also adds another variant which is a boxed `MockStream`, making
    the smtp libraries, but also libraries build on top of it more
    testable.

3. `mock::MockStream` (use the features `mock-impl`)
    A simple implementation for a `MockStream` which allows you
    to test which data was send to it and mock responses for it.
    (Through it's currently limited to a fixed predefined conversation,
    if more is needed a custom `MockStream` impl. has to be used)

4. `future_ext::ResultWithContextExt`:
    Provides a `ctx_and_then` and `ctx_or_else` methods making
    it easier to handle results resolving _as Item_ to an tuple
    of a context (here the connection) and a `Result` belonging
    to an different abstraction level than the futures `Error`
    (here a possible `CommandError` while the future `Error` is
    an connection error like e.g. a broken pipe)

Limitations / TODOs
--------------------

Like mentioned before this library has some limitations as it's
meant to _only_ do SMTP and nothing more. Through there are
some other limitations, which will be likely to be fixed
in future versions:

1. no mail address parser for `send_mail::MailAddress` and neither
   a parser for `ForwardPath`/`ReversePath` (they can be constructed
   using `from_unchecked`).  This will be fixed when I find a library
   "just" doing mail addresses and doing it right.

2. no "build-in" support for extended status codes, this is mainly
   the way because I hadn't had time for this, changing this in a nice
   build-in way might require some API changes wrt. to the
   `Response` type and it should be done before `v1.0`

3. The number of provided commands is currently limited to a
   small but useful subset, commands which would be nice
   to provide include `BDAT` and more variations of `AUTH`
   (currently provided are `PLAIN` and simple `LOGIN` which
    is enough for most cases but supporting e.g. `OAuth2` would
    be good)

4. no support for `PIPELINING`, while most extensions can be
   implemented using custom commands, this is not true for
   pipelining. While there exists a concept how pipelining
   can be implemented without to much API brakeage this is
   for now not planed due to time limitations.

5. no stable version (`v1.0`) for now, as `tokio` is not stable yet.
   When tokio becomes stable a stable version should be released,
   through another one might have to be released at some point if
   `PIPELINING` is implemented later one (through in the
   current concept for implementing it there are little
   braking changes, except for implementors of custom commands)

Documentation
--------------

Documentation can be [viewed on docs.rs](https://docs.rs/new-tokio-smtp).

License
--------

Licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

Contribution
-------------

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.

Change Log
----------

- `v0.4`:
  - renamed `from_str_unchecked` to `from_unchecked`
  - `Cmd.exec` accepts now `Io` instead of `Connection`
    - replace `CmdFuture` with `ExecFuture`
    - `Connection.send_simple_cmd` is now `Io.exec_simple_cmd`

- `v0.5`:
  - improved `ClientId`
    - renamed `ClientIdentity` to `ClientId`
    - added `hostname()` constructor
  - added builder for `ConnectionConfig`
    - removed old `with_` constructors
  - placed all `Auth*` commands into a `auth` module
    (e.g. `AuthPlain` => `auth::Plain`)
  - changed feature naming schema

- `v0.6`
  - added connection builder for local non secure connections
  - addec constructors for builders on types they build
  - renamed `auth::plain::NullCodePoint` to `auth::plain::NullCodePointError`

- `v0.7`
  - `send_all_mails` and `connect_send_quit` now accept a `IntoIterable` instead of stream
    - you need to have all values already ready when sending so `Stream` didn't fit well
    - it also means you can now pass in a `Vec` or a `std::iter:once`
  - `GeneralError` as no longer the `PreviousRequestKilledConnection` error variant instead
    a `std::io::Error::new(std::io::ErrorKind::NoConnection, "...")` is returned which makes
    it easier to adapt to by libraries using it and fit the semantics as good as previous
    solution
  - `send_all_mails` and `connect_send_quit` now return a stream instead of a future resolving
    to a stream.

- `v0.7.1`
  - updated dependencies, makes sure tokio doesn't produces a deprecation warning
    as a import moved to a different place in tokio

- `v0.8.0`
  - update `tokio-tls`/`native-tls` to v0.2.x
  - renamed method creating a builder from `build*` to `builder*`

- `v0.8.1`
  - `SelectCmd` and `EitherCmd` where added

- `v0.8.2`
  - Fix bug where the wrong parsing error was emitted for
    `EsmtpValue`


Contributors
-------------

- katyo (https://github.com/katyo)