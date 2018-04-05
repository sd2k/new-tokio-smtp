# Pre Glossary

## AsyncSeq: 

Operations which are Async but do not "fork" by e.g. spawning in a cpu pool.
They tend to be a future and_then chain.

The next operation can only be started when the previous was completed

## AsyncDelegate: 

Operations which delegate work to a different place by sending think
to another place (i.e. delegating the work). Potentially using multiple
threads, potentially loasing sequency of operation, potentially hard to
represent in a simple activit diagram

It might be possible to start the next action before the previous one completed.

## Async:

can bei any Async* case, e.g. AsyncSeq

# Components for SMTP

## Start Connection

Kind: AsyncSeq
DependsOn: `Tcp`, `Tls`
Parameters: Server Ip, Opt. sni name
Output: `SMTP Cmd Receiver [secure?]` 

```ascii
 ____________________________________________________________________________________________
/  Start Connection  |                                                                       \
|____________________|                                                                        |
|                                                                                             |
|  Start -> Connecting Tcp -> Unsecure -> EHLO -> Unsecure With EHLO -> START TLS \           |
|        \__-> Connectin Direct TLS -> Secure \         |                          |          |
|              _-<-EHLO <--___________________/         |                          |          |
|             /                      __________________/                           |          |
|   ________-+-____________________-+-_____________________________________________/          |     
|  /         |                      |                                                         |
|  \_-> Secure with EHLO ->  SMTP Cmd Receiver [secure?]                                      |
|                                                                                             |
\____________________________________________________________________________________________/ 
```


## SMTP Cmd Receiver State

```ascii

State {
    io: enum {
        Secure(TlsConnection),
        Insecure(TcpConnection)
    },
    ehlo: EhoData
}

```

## SMTP Commands


### EHLO

- not allowed, we do this on connect ourself

### STARTTLS

DependsOn: `Tls`
Requires State:  `State.io` is `Insecure`
Changes: `State.io` to `Secure`
Parameter: Opt. but recommended: sni name

### Mail
 

### Auth

Starts Sub Communication: True

### Data

Starts Sub Communication: True


## Sending a Command


```ascii
    send command -> result code --err_code--> end [error]
                         |--ok_code--> end [ok]
                         |--intermediate_code:not(cmd.has_intermediate)--> end [fault]
                         \--intermediate_code:cmd.has_intermediate--\
          __________________________________________________________/
         /                                                      ____________
         \_-> intermediate handle (code) --use_subcommand--> __/ sub rotine \___ --> end [*]
                     |                                         \____________/         |
                     |                                                                |
                     \--send_raw[writer/reader]--> writer.send(state.io as IoStream)-/
```

`send_raw` can not be limited to bit staching, there are _alternate_ data formats



# Rust Scatch

```rust

struct Io {
	conn: MaybeSecure,
	buffer: Buffers
}
struct SmtpIo {
	io: Io,
	ehlo: EhloData,
}


impl SmtpIo {

	fn cmd<C: SmtpCmd>(self, cmd: C) -> CmdFuture { /*...*/ }
}

trait SmtpCmd {
	fn write_cmd(buf: &mut SomeWriterType) -> SomeResult;
	
	fn hanlde_intermediate(self, resp: SmtpResponse, io: Io) -> Option<Box<Future<Item=(Result<SmtpResponse, LogicError>, Io), Error=io::Error>>> {			None
	}
}
```
