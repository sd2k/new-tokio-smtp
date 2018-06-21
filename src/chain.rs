//! Provides the `smtp_chain` macro and the `chain` function
//!
//! see their respective documentation for more information.
use std::io as std_io;
use std::sync::Arc;
use futures::future::{self, Future, Loop, Either};

use ::{command, Connection, BoxedCmd};
use ::error::LogicError;

/// creates a chain of commands and them to the given connection
///
/// This will call `boxed()` on every command, while puting them
/// into a vector which is then passed to `cain`.
///
/// # Example
///
/// ```no_run
/// # extern crate futures;
/// # #[macro_use] extern crate new_tokio_smtp;
/// use futures::future::{self, Future};
/// use new_tokio_smtp::chain::OnError;
/// use new_tokio_smtp::{command, Connection, ReversePath, ForwardPath};
///
///
/// let fut = future
///     ::lazy(|| mock_create_connection())
///     .and_then(|con| smtp_chain!(con with OnError::StopAndReset => [
///         command::Mail::new(
///             ReversePath::from_unchecked("test@sender.test")),
///         command::Recipient::new(
///             ForwardPath::from_unchecked("test@receiver.test")),
///         command::Data::from_buf(concat!(
///             "Date: Thu, 14 Jun 2018 11:22:18 +0000\r\n",
///             "From: Sendu <test@sender.test>\r\n",
///             "\r\n",
///             "...\r\n"
///         ))
///     ]))
///     .and_then(|(con, smtp_chain_result)| {
///         if let Err((at_idx, err)) = smtp_chain_result {
///             println!("server says no on the cmd with index {}: {}", at_idx, err)
///         }
///         con.quit()
///     });
///
/// // ... this are tokio using operation make sure there is
/// //     a running tokio instance/runtime/event loop
/// mock_run_with_tokio(fut);
///
/// # // some mock-up, for this example to compile
/// # fn mock_create_connection() -> Result<Connection, ::std::io::Error>
/// #  { unimplemented!() }
/// # fn mock_run_with_tokio(f: impl Future) { unimplemented!() }
///
/// ```
#[macro_export]
macro_rules! smtp_chain {
    ($con:ident with $oer:expr => [$($cmd:expr),*]) => ({
        use $crate::Cmd;
        $crate::chain::chain($con, vec![$($cmd.boxed()),*], $oer)
    });
}

/// implement this trait for custom error in chain handling
///
/// e.g. a smtp allows failing some of the `RCPT` command in
/// a single mail transaction. The default handling won't allow
/// this but a custom implementation could.
pub trait HandleErrorInChain: Send + Sync + 'static {
    type Fut: Future<Item=(Connection, bool), Error=std_io::Error> + Send;

    /// handle the error on given connection for the `msg_idx`-ed command
    /// given the given logic error
    fn handle_error(&self, con: Connection, msg_idx: usize, logic_error: &LogicError)
        -> Self::Fut;
}

/// send all commands in `chain` through the given connection one
/// after another
pub fn chain<H>(con: Connection, chain: Vec<BoxedCmd>, on_error: H)
    -> impl Future<Item=(Connection, Result<(), (usize, LogicError)>), Error=std_io::Error> + Send
    where H: HandleErrorInChain
{
    let _on_error = Arc::new(on_error);
    let mut chain = chain;
    //stackify
    chain.reverse();

    // the index of the current operation in the chain plus 1
    let mut index_p1 = 0;
    let fut = future
        ::loop_fn(con, move |con| {
            index_p1 += 1;
            if let Some(next_cmd) = chain.pop() {
                //FIXME[rust/co-rotines+self-borrow]: this is likly not needed with self borrow
                let on_error = _on_error.clone();
                let fut = con
                    .send(next_cmd)
                    .and_then(move |(con, result)| match result {
                        Ok(_result) => {
                            Either::A(future::ok(Loop::Continue(con)))
                        },
                        Err(err) => {
                            let index = index_p1 - 1;
                            let fut = on_error
                                .handle_error(con, index, &err)
                                .map(move |(con, stop)| {
                                    if stop {
                                        Loop::Break((con, Err((index, err))))
                                    } else {
                                        Loop::Continue(con)
                                    }
                                });
                            Either::B(fut)
                        }
                    });

                Either::A(fut)
            } else {
                Either::B(future::ok(Loop::Break((con, Ok(())))))
            }
        });

    fut
}

/// Decide if a error should just stop sending commands or should
/// also trigger the sending of `RSET` stopping the current mail
/// transaction
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum OnError {
    StopAndReset,
    Stop
}

impl HandleErrorInChain for OnError {
    //FIXME[rust/impl Trait for associated type]: use impl Trait/abstract type
    type Fut = Box<Future<Item=(Connection, bool), Error=std_io::Error> + Send>;
    //type Fut = impl Future<Item=(Connection, bool), Error=std_io::Error> + Send;

    fn handle_error(&self, con: Connection, _msg_idx: usize, _error: &LogicError) -> Self::Fut {
        let fut = match *self {
            OnError::Stop => Either::A(future::ok((con, true))),
            OnError::StopAndReset => {
                let fut = con
                    .send(command::Reset)
                    //Note: Reset wont reach (con, Err(_)), ever! a reset error is turned
                    // into a io::Error
                    .map(|(con, _)| (con, true));

                Either::B(fut)
            }
        };

        Box::new(fut)
    }
}