use std::io as std_io;
use std::sync::Arc;
use futures::future::{self, Future, Loop, Either};

use ::{command, Connection, BoxedCmd};
use ::error::LogicError;


#[macro_export]
macro_rules! smtp_chain {
    ($con:ident with $oer:expr => [$($cmd:expr),*]) => ({
        use $crate::Cmd;
        $crate::chain::chain($con, vec![$($cmd.boxed()),*], $oer)
    });
}

pub trait HandleErrorInChain: Send + Sync + 'static {
    type Fut: Future<Item=(Connection, bool), Error=std_io::Error> + Send;

    fn handle_error(&self, con: Connection, msg_idx: usize, logic_error: &LogicError) -> Self::Fut;
}


//FIXME[rust/impl Trait in struct]: use impl Trait/abstract type
pub fn chain<H>(con: Connection, chain: Vec<BoxedCmd>, on_error: H)
    -> Box<Future<Item=(Connection, Result<(), (usize, LogicError)>), Error=std_io::Error> + Send>
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

    Box::new(fut)
}


#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum OnError {
    StopAndReset,
    Stop
}

impl HandleErrorInChain for OnError {
    //FIXME[rust/impl Trait for associated type]: use impl Trait/abstract type
    type Fut = Box<Future<Item=(Connection, bool), Error=std_io::Error> + Send>;

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