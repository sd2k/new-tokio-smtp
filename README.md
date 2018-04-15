
# new-tokio-smtp &emsp; 
[![docs](https://docs.rs/new-tokio-smtp/badge.svg)](https://docs.rs/new-tokio-smtp) 
[![Build Status](https://travis-ci.org/1aim/new_tokio_smtp.svg?branch=master)](https://travis-ci.org/1aim/new_tokio_smtp)


**The new-tokio-smtp crate provides an extendible SMTP (Simple Mail Transfer Protocol)
implementation using tokio.**

---

This crate provides _only_ SMTP functionality, this means it does neither
provides functionality for creating mails, nor for e.g. retrying sending
a mail if the reciver was temporary not aviable.

While it only provides SMTP functionality it is written in a way to
make it easy to integrate with higher level libraries. The interoperability
is provided through two mechanisms:

1. SMTP commands are defined in a way which allow library user to
   define there own commands, all commands provided by this libary
   could theoretically have been implemented in an external libary,
   this includes some of the more special commands like `STARTTLS`, 
   `EHLO` and `DATA`. Moreover a `Connection` can be converted into
   a `Io` instance which provides a number of usefull functionalities
   for easily implementing new commands, e.g. `Io.parse_response`.

2. syntactic construct's like e.g. `Domain` or `ClientIdentity` can
   be parsed but also have "unchecked" constructors, this allows libraries
   which have there own validation to skip redundant validations, e.g.
   if a mail libary might provide a `Mailbox` type of mail addresses and
   names, which is guranteed to be syntactically correct if can implement
   a simple `From`/`Into` impl to cheaply convert it to an `Forward-Path`.
   (Alternative thry also could implement their own `Mail` cmd if this
   has any benefit for them)

3. provided commands (and syntax constructs) are written in a robust way,
   allowing for example extensions like `SMTPUTF8` to be implemented on it.
   The only drawback of this is that it trusts that parts created by more
   higher level libaries are valid, e.g. it won't validate that the mail
   given to it is actually 7bit ascii or that it does not contain "orphan"
   `'\n'` (or `'\r'`) chars. But this is fine as this libary is for using
   smtp to send mails, but not for creating such mails. (Note that while
   it is trusting it does validate if a command can be used through checking
   the result from the last ehlo command, i.e. it wont allow you to send
   a `STARTTLS` command on a mail server not supporting it)

4. handling logic errors (i.e. server responded with code 550) seperatly
   from more fatal errors like e.g. a broken pipe


# Example

TODO impl example based on the _non-public_ smtp-send bin/carte


# Concept

The concept of behind the library is explained
in the [notes/concept.md](./notes/convept.md) file.

TODO short description


# Usability Helpers

The libary provides a number of usability helpers:

1. `chain::chain` provides a easy way to chain a number of
    SMTP commands, sending each command when the previous
    command in the chain did not fail in any way.

2. `mock_support` feature:
    Extends the Socket abstration to not only abstract over
    the socket beeing either a `TcpStream` or a `TlsStream` but
    also adds another variant which is a boxed `MockStream`, making
    the smtp libraries, but also libraries build on top of it more
    testable.

3. `mock::MockStream` (use the features `mock_impl`)
    A simple implementation for a `MockStream` which allows you
    to test which data was send to it and mock responses for it.
    (Through it's currently limited to a fixed predefined conversation,
    if more is needed a custom `MockStream` impl. has tobe used) 

4. `future_ext::ResultWithContextExt`: 
    Provids a `ctx_and_then` and `ctx_or_else` methods making
    it easier to handle results resolving _as Item_ to an tuple
    of a context (here the connection) and a `Result` belonging
    to an different abstraction level than the futures `Error` 
    (here a possible `CommandError` while the future `Error` is
    an connection error like e.g. a broken pipe)


# Limitations / TODOs

Like mentioned before this libary has some limitations as it's
meant to _only_ do SMTP and nothing more. Through there are
some other limitations, which will be likely to be fixed
in future versions:

1. no mail address parser, `ForwardPath` and `ReversePath` can
   only be constructed using `from_str_unchecked` (for now).
   This will be fixed when I find a library "just" doing mail
   addresses and doing it right

2. no "build-in" support for extended status codes, this is mainly
   the way because of time limitations, changing this in a nice
   build-in way might require some API changes wrt. to the
   `Response` type and it should be done before `v1.0`

3. The number of provided commands is currently limited to a
   small but usefull subset, commands which would be nice
   to provide include `BDAT` and more variations of `AUTH`
   (currently provided are `PLAIN` and simple `LOGIN`)

4. no support for `PIPELINING`, while most extensions can be
   implemented using custom commands, this is not true for
   pipelining. While there exists a concept how pipelining
   can be implemented without to much API brakage this is
   for now not planed due to time limitations.

5. no stable version (`v1.0`) for now, as `tokio` is not stable yet.
   When tokio becomes stable a stable version should be released, 
   through another one might have to be released at some point if
   `PIPELINING` is implemented later one (through in the
   current concept for implementing it there are little
   braking changes, except for implementors of custom commands)


# Documentation

Documentation can be [viewed on docs.rs](https://docs.rs/new-tokio-smtp).


## License

Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
