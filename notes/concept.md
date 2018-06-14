
# Concept

This library provides to "perspectives", the one
from the user of the library and the one from the
"command" used with this library (and often also
provided by it.)

## User Perspective

From the user perspective the library has roughly
following flow (ignoring the part about needing to
do so "in"/"with" a tokio event loop):

1. create a `ConnectionConfig`
2. create a `Connection` using `connect`
3. send a `Cmd` i.e. call `connection.send(cmd)` with any type implementing `Cmd`
4. get a future and wait for it to complete
5. potentially go back to step 3. and send another `Cmd`
6. call `quit` and complete the resulting future

Notable are three aspects:

1. each `Cmd` is it's own type:
   - This allows the extension of the library with new commands
    without needing any modifications to it's interner, so anyone
    can define new `Cmd`'s _outside_ of this crate/libary and use
    them in a type safe way.
   - This has only the drawback that you can't have a `Vec` of `Cmd`'s
    which is fine for many situation, if it isn't `cmd.boxed()` can be
    used to get something like a boxed command (it's not directly a
    `Cmd` trait object as `Cmd` is not object safe but it works like
    just that)

2. calling `connection.send(cmd)` _consumes_ the connection, returning
   a future which returns the connection once it's resolved
   - This makes sure that you can just send one command at a times
    over the same connection, it also prevent you from trying to send
    commands over dead connections (at last in most cases)

3. The future of `connection.send(cmd)` resolves to a `Result` of an `Result`
   (`Result<(Connection, Result<Response, LogicError>), io::Error>)`
   - This is a consequence of **there being two fundamentally different kinds of
    errors: connection level error and "logic" level errors**
   - The outer I/O-Error happens if the connection to the server dies
    for one reason or another. In this case we can't use the `Connection` any longer
    and thus don't return it. We also do not get an error code.
   - The inner `LogicError` represents that the command was successfully send to
    the server and a result was successfully parsed (i.e. this library did it's job)
    but the parsed result code indicates that the command was erroneous, e.g. the
    targets mailbox was not found. In this case the connection is still fine and
    we can continue to use it, so we return it together with the result indicated
    through the response code of the server
   - for easier chaining thinks only while both the other result is ok and the inner
    result (i.e. the response code) are ok you can use the `ResultWithContextExt`
    traits `ctx_and_then`, `ctx_or_else` methods.

## Command Perspective

While the user just sends commands to and connection from pov. of the
commands thinks are slightly more complex. Notably while a lot commands
simple send some text and then wait for an response some are more complext:

- some commands modify or even fundamentally change the Connection,
  e.g. STARTTLS switches to an TLS connection and `EHLO` set's the ehlo data of
  the `Connection` instance

- some commands have one or multiple intermediate responses they have to
  react to going as far as having "sub conversations".

- most commands send a simple line, but some send more text with a complex
  eoi condition (e.g. `DATA`) or send fixed sized of binary chunks (e.g. `BDAT`)

Because of this and the fact that this library is meant to be extendible it
provides more or less a toolbox to interact with an smtp server for commands.
This includes functionality to:

- "send"/"flush" arbitrary data
- input/output buffer management
- parse a standard smtp response
- send dot-stashed data
- abstract over the actual socket type of either Tcp, TlsTcp or Mock. Where
  the later one is hidden behind a feature and meant for testing.
- check if the command is supported (using the EHLO data)

Nevertheless non of the methods provided for this should be visible for the
"normal" library user. As such the `Connection` type used normally is actually
just a wrapper type around a `Io` type providing all the low level methods.
If `connection.send(cmd)` is called it will check if the command can be used
given the ehlo data and if so passes itself to `cmd.exec(con)` where the cmd
then turns it into from an `Connection` instance into a `Io` instance it can
use. While this seems a bit round about any calling overhead is (should be)
optimized away by the compiler allowing us to have two different clean interfaces
for either simple api user or cmd implementor.