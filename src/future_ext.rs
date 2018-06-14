use futures::{Future, IntoFuture, Poll, Async};

/// A helper trait implemented on Futures
///
/// This is implemented on futures which resolve to an
/// item of the form `(Ctx, Result<Item, Err>)` and adds
/// chaining methods which are based on the result inside
/// the item instead of the result the future resolves to
pub trait ResultWithContextExt<Ctx, I, E>: Future<Item=(Ctx, Result<I, E>)> {

    /// use this to chain based on the Ctx and _inner_ item in a future
    /// like `Future<(Ctx, Result<Item, Err>), Err2>`
    ///
    /// given a `Future<(Ctx, Result<Item, Err>), Err2>` this:
    ///
    /// 1. resolves forwards the result of resolving the future if
    ///     1. the future resolves to err (Err2)
    ///     2. the inner result is a error (Err)
    /// 2. calls `f(ctx, item)` if the inner result is Ok
    ///     note that the result of f has to be convertible into an
    ///     future of the impl `Future<(Ctx, Result<Item2, Err>), Err2>`
    fn ctx_and_then<FN, B, I2>(self, f: FN) -> CtxAndThen<Self, B, FN>
        where FN: FnOnce(Ctx, I) -> B,
              B: IntoFuture<Item=(Ctx, Result<I2, E>), Error=Self::Error>,
              Self: Sized;

    /// use this to chain based on the Ctx and _inner_ error in a future
    /// like `Future<(Ctx, Result<Item, Err>), Err2>`
    ///
    /// given a `Future<(Ctx, Result<Item, Err>), Err2>` this:
    ///
    /// 1. resolves forwards the result of resolving the future if
    ///     1. the future resolves to err (Err2)
    ///     2. the inner result is ok (Err)
    /// 2. calls `f(ctx, err)` if the inner result is err
    ///     note that the result of f has to be convertible into an
    ///     future of the impl `Future<(Ctx, Result<Item, Err3>), Err2>`
    fn ctx_or_else<FN, B, E2>(self, f: FN) -> CtxOrElse<Self, B, FN>
        where FN: FnOnce(Ctx, E) -> B,
              B: IntoFuture<Item=(Ctx, Result<I, E2>), Error=Self::Error>,
              Self: Sized;
}

impl<Ctx, I, E, FUT> ResultWithContextExt<Ctx, I, E> for FUT
    where FUT: Future<Item=(Ctx, Result<I, E>)>
{
    fn ctx_and_then<FN, B, I2>(self, f: FN) -> CtxAndThen<Self, B, FN>
        where FN: FnOnce(Ctx, I) -> B,
              B: IntoFuture<Item=(Ctx, Result<I2, E>), Error=Self::Error>,
              Self: Sized
    {
        CtxAndThen {
            state: State::Parent(self),
            map_fn: Some(f)
        }
    }

    fn ctx_or_else<FN, B, E2>(self, f: FN) -> CtxOrElse<Self, B, FN>
        where FN: FnOnce(Ctx, E) -> B,
              B: IntoFuture<Item=(Ctx, Result<I, E2>), Error=Self::Error>,
              Self: Sized
    {
        CtxOrElse {
            state: State::Parent(self),
            map_fn: Some(f)
        }
    }

}

enum State<P, IM> {
    Parent(P),
    Intermediate(IM),
}

/// future adapter see `ResultWithContextExt::ctx_and_then`
pub struct CtxAndThen<P, B, FN>
    where B: IntoFuture,

{
    state: State<P, B::Future>,
    map_fn: Option<FN>
}

impl<P, B, FN, Ctx, I, I2, E> Future for CtxAndThen<P, B, FN>
    where P: Future<Item=(Ctx, Result<I, E>)>,
          FN: FnOnce(Ctx, I) -> B,
          B: IntoFuture<Item=(Ctx, Result<I2, E>), Error=P::Error>,
{
    type Item = (Ctx, Result<I2, E>);
    type Error = P::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let (ctx, result) = match self.state {
            State::Parent(ref mut p) => {
                try_ready!(p.poll())
            },
            State::Intermediate(ref mut im) => {
                return im.poll();
            },
        };

        let item = match result {
            Err(err) => return Ok(Async::Ready((ctx, Err(err)))),
            Ok(item) => item
        };

        let map_fn = self.map_fn.take().expect("polled after completion/panic");
        let bval = (map_fn)(ctx, item);
        let mut fut = bval.into_future();
        let first_poll_res = fut.poll();
        self.state = State::Intermediate(fut);

        first_poll_res
    }
}

//FIXME[dry]: dedup code between CtxOrElse/CtxAndThen
// (macro or something like ctx_on_inner(get_from_result_fn, map_stuf_you_got_fn))
/// future adapter see `ResultWithContextExt::ctx_or_else`
pub struct CtxOrElse<P, B, FN>
    where B: IntoFuture,

{
    state: State<P, B::Future>,
    map_fn: Option<FN>
}

impl<P, B, FN, Ctx, I, E, E2> Future for CtxOrElse<P, B, FN>
    where P: Future<Item=(Ctx, Result<I, E>)>,
          FN: FnOnce(Ctx, E) -> B,
          B: IntoFuture<Item=(Ctx, Result<I, E2>), Error=P::Error>,
{
    type Item = (Ctx, Result<I, E2>);
    type Error = P::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let (ctx, result) = match self.state {
            State::Parent(ref mut p) => {
                try_ready!(p.poll())
            },
            State::Intermediate(ref mut im) => {
                return im.poll();
            },
        };

        let err = match result {
            Ok(item) => return Ok(Async::Ready((ctx, Ok(item)))),
            Err(err) => err
        };

        let map_fn = self.map_fn.take().expect("polled after completion/panic");
        let bval = (map_fn)(ctx, err);
        let mut fut = bval.into_future();
        let first_poll_res = fut.poll();
        self.state = State::Intermediate(fut);

        first_poll_res
    }
}


#[cfg(test)]
mod test {

    mod ctx_and_then {
        use std::io::{Error, ErrorKind};
        use futures::future::{self, Future};
        use super::super::*;

        #[test]
        fn map_outer_err() {
            let fut = future::err::<(String, Result<u8, String>), Error>(
                Error::new(ErrorKind::Other, "test")
            );

            let res = fut
                .ctx_and_then(|_ctx, _item| -> Result<(_, Result<String, _>), _> {
                    unreachable!()
                })
                .wait();

            assert!(res.is_err());
        }

        #[test]
        fn map_inner_ok_with_ok() {
            let fut = future::ok::<(String, Result<u8, String>), Error>(("14".to_owned(), Ok(2)));

            let res = fut
                .ctx_and_then(|ctx, item| {
                    let mul: u8 = ctx.parse().unwrap();
                    let res = item * mul;
                    Ok(("changed".to_owned(), Ok(format!("bla: {}", res))))
                })
                .wait()
                .unwrap();

            assert_eq!(res, ("changed".to_owned(), Ok("bla: 28".to_owned())));
        }

        #[test]
        fn map_inner_err() {
            let fut = future::ok::<(String, Result<u8, String>), Error>(
                ("14".to_owned(), Err("err".to_owned())));

            let res = fut
                .ctx_and_then(|ctx, item| {
                    let mul: u8 = ctx.parse().unwrap();
                    let res = item * mul;
                    Ok(("changed".to_owned(), Ok(format!("bla: {}", res))))
                })
                .wait()
                .unwrap();

            assert_eq!(res,  ("14".to_owned(), Err("err".to_owned())));
        }

        #[test]
        fn map_inner_ok_with_err() {
            let fut = future::ok::<(String, Result<u8, String>), Error>(("14".to_owned(), Ok(2)));

            let res = fut
                .ctx_and_then(|ctx, item| {
                    if /*false*/ item > 128 {
                        Ok((ctx, Ok(0u32)))
                    } else {
                        Ok((ctx, Err("failed".to_owned())))
                    }
                })
                .wait()
                .unwrap();

            assert_eq!(res, ("14".to_owned(), Err("failed".to_owned())));
        }
    }
}