use std::io::{self as std_io, Read, Write};
use std::thread;
use std::time::Duration;
use std::mem;
use std::cmp::min;

use rand::{random, thread_rng, Rng};

use bytes::{BytesMut, BufMut};
use futures::{future, Future, Poll, Async, Stream};
use futures::task::{self, Task};
use futures::sync::mpsc;
use tokio::io::{AsyncRead, AsyncWrite};


use ::io::MockStream;

#[derive(Debug)]
pub enum Actor {
    Server,
    Client
}

#[derive(Debug)]
pub enum ActionData {
    Lines(Vec<&'static str>),
    Blob(Vec<u8>)
}

impl ActionData {

    fn len(&self) -> usize {
        match *self {
            ActionData::Blob(ref blob) => blob.len(),
            ActionData::Lines(ref lines) => {
                //MAGIC_NUM: +2 = "\r\n".len()
                lines.iter().map(|ln| ln.len() + 2).sum()
            }
        }
    }

    fn assert_same_start(&self, other: &[u8]) {

        match *self {
            ActionData::Blob(ref blob) => {
                let use_len = min(blob.len(), other.len());
                let other = &other[..use_len];
                let blob = &blob[..use_len];
                //TODO better error message (assert_eq is a BAD idea here)
                assert!(blob == other, "unexpected data");
            },
            ActionData::Lines(ref lines) => {
                let mut rem = other;
                for line in lines.iter() {
                    let use_len = min(line.len(), rem.len());
                    let use_of_line = &line[..use_len];
                    let other = &rem[..use_len];
                    //TODO better error message (assert_eq is a BAD idea here)
                    assert!(use_of_line.as_bytes() == other, "unexpected data");

                    if use_len < line.len() {
                        // we could just check a part of a line,
                        // so we need more data => brake
                        break;
                    }

                    let other = &other[use_len..];

                    //check the "\r\n" omitted in Lines
                    assert!(other.starts_with(b"\r\n"), "unexpected data");
                    rem = &other[2..];
                }
            }
        }
    }
}

type Waker = mpsc::UnboundedSender<Task>;

#[derive(Debug)]
enum State {
    ServerIsWorking {
        waker: Waker,
        to_be_read: BytesMut
    },
    ClientIsWorking {
        expected: ActionData,
        waker: Waker,
        input: BytesMut
    },
    NeedNewAction {
        waker: Waker,
        buffer: BytesMut
    },
    ShutdownOrPoison
}

impl State {

    fn waker(&self) -> &Waker {
        match *self {
            State::ServerIsWorking { ref waker, ..} => waker,
            State::ClientIsWorking { ref waker, ..} => waker,
            State::NeedNewAction { ref waker, ..} => waker,
            _ => panic!("trying to shedule wakup on shutdown stream")
        }
    }
}

#[derive(Debug)]
pub struct MockSocket {
    conversation: Vec<(Actor, ActionData)>,
    fake_secure: bool,
    state: State
}


impl MockSocket {

    pub fn new(conversation: Vec<(Actor, ActionData)>) -> Self {
        let mut conversation = conversation;
        //queue => stack
        conversation.reverse();

        MockSocket {
            conversation,
            fake_secure: false,
            state: State::NeedNewAction {
                buffer: BytesMut::new(),
                waker: delayed_waker()
            },
        }
    }

    pub fn no_assert_drop(mut self) {
        self.state = State::ShutdownOrPoison;
        mem::drop(self);
    }

    fn schedule_delayed_wake(&mut self) {
        self.state.waker()
            .unbounded_send(task::current())
            .unwrap()
    }

    fn maybe_inject_not_ready(&mut self) -> Poll<(), std_io::Error> {
        // 1/16 chance to be not ready
        if random::<u8>() >= 240 {
            self.schedule_delayed_wake();
            Ok(Async::NotReady)
        } else {
            Ok(Async::Ready(()))
        }
    }

    fn prepare_next(&mut self, waker: Waker, buffer: BytesMut) -> State {

        let (actor, data) = self.conversation.pop()
            .expect("prepare next on empty conversation");

        let mut buffer = buffer;

        match actor {
            Actor::Server => {
                // 1. data into() buffer
                assert!(buffer.is_empty(), "buffer had remaining input");
                buffer.reserve(data.len());
                match data {
                    ActionData::Lines(lines) => {
                        for line in lines {
                            buffer.put(line);
                            buffer.put("\r\n");
                        }
                    },
                    ActionData::Blob(blob) => {
                        buffer.put(blob);
                    }
                }
                State::ServerIsWorking {
                    waker,
                    to_be_read: buffer
                }
            },
            Actor::Client => {
                // 1. clear buffer / reserve space in buffer
                State::ClientIsWorking {
                    expected: data,
                    waker,
                    input: buffer
                }
            }
        }
    }


}

impl Drop for MockSocket {

    fn drop(&mut self) {
        if !thread::panicking() {
            if let State::ShutdownOrPoison = self.state {}
            else { panic!("connection was not shutdown"); }

            assert!(self.conversation.is_empty(), "premeature cancelation of conversation");
        }
    }
}

impl MockStream for MockSocket {
    fn is_secure(&self) -> bool {
        self.fake_secure
    }

    fn set_is_secure(&mut self, secure: bool) {
        self.fake_secure = secure;
    }
}

// before read/write:
//   read/write --> state == NeedNewAction --> prepare next action
//
// on read:
//   read --> Actor == Server -> part of  buffer into read -> return bytes transmitted
//        |                                 \-> state = NeedNewAction
//        |
//        \-> Actor == Client -> would Block / panic?
//
// on write:
//   write --> Actor == Client -> read from buffer -> return ...
//         |                          \-> if "end condition"
//         |                                \-> assert read input == expected input
//         |                                       \-> state = NeedNewAction
//         |
//         \-> Actor == Server -> would Block / panic?
//
// "end condition"
//    1st: read length >= expected read length
//    2nd: alt condition "\r\n.\r\n" read?
//
// inject NotReady return:
//   on before read
//   on read after transmitting >= 1 byte
//   on before write
//   on write after trasmitting >= 1 byte
//
// on NotReady return:
//   send Task to DelayedWakerThread

macro_rules! try_ready_or_would_block {
    ($expr:expr) => ({
        let res = $expr;
        match res {
            Ok(Async::Ready(t)) => t,
            Ok(Async::NotReady) => {
                return Err(std_io::Error::new(std_io::ErrorKind::WouldBlock, "Async::NotReady"));
            },
            Err(err) => {
                return Err(err);
            }
        }
    });
}

impl Read for MockSocket {

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std_io::Error> {
        Ok(try_ready_or_would_block!(self.poll_read(buf)))
    }
}

impl Write for MockSocket {

    fn write(&mut self, buf: &[u8]) -> Result<usize, std_io::Error> {
        Ok(try_ready_or_would_block!(self.poll_write(buf)))
    }

    fn flush(&mut self) -> Result<(), std_io::Error> {
        Ok(try_ready_or_would_block!(self.poll_flush()))
    }
}

impl AsyncRead for MockSocket {

    fn poll_read(&mut self, buf: &mut [u8]) -> Poll<usize, std_io::Error> {
        try_ready!(self.maybe_inject_not_ready());
        let state = mem::replace(&mut self.state, State::ShutdownOrPoison);
        match state {
            State::ShutdownOrPoison => {
                panic!("tried reading from shutdown/poisoned stream")
            },
            State::ClientIsWorking { .. } => {
                panic!("tried to read from socket while it should only write to it")
            },
            State::NeedNewAction { waker, buffer } => {
                self.state = self.prepare_next(waker, buffer);
                self.schedule_delayed_wake();
                Ok(Async::NotReady)
            }
            State::ServerIsWorking { waker, mut to_be_read } => {
                let rem = to_be_read.len();
                let can_write = buf.len();
                let should_write = random_amount(min(rem, can_write));

                write_n_to_slice(&to_be_read, buf, should_write);
                to_be_read.advance(should_write);

                if to_be_read.is_empty() {
                    self.state = State::NeedNewAction { waker, buffer: to_be_read }
                } else {
                    self.state = State::ServerIsWorking { waker, to_be_read }
                }
                Ok(Async::Ready(should_write))
            },
        }
    }
}

impl AsyncWrite for MockSocket {

    fn poll_write(&mut self, buf: &[u8]) -> Poll<usize, std_io::Error> {
        try_ready!(self.maybe_inject_not_ready());
        let state = mem::replace(&mut self.state, State::ShutdownOrPoison);
        match state {
            State::ShutdownOrPoison => {
                panic!("tried reading from shutdown/poisoned stream")
            },
            State::ServerIsWorking { .. } => {
                panic!("tried to write to socket while it should only read from it")
            },
            State::NeedNewAction { waker, buffer } => {
                self.state = self.prepare_next(waker, buffer);
                self.schedule_delayed_wake();
                Ok(Async::NotReady)
            }
            State::ClientIsWorking { expected, waker, mut input } => {
                let amount = random_amount(buf.len());
                if input.remaining_mut() < amount {
                    input.reserve(amount)
                }
                let actual_write = buf.split_at(amount).0;
                input.put(actual_write);

                self.state = State::ClientIsWorking { expected, waker, input };
                Ok(Async::Ready(amount))
            }
        }
    }

    fn poll_flush(&mut self) -> Poll<(), std_io::Error> {
        try_ready!(self.maybe_inject_not_ready());
        let state = mem::replace(&mut self.state, State::ShutdownOrPoison);
        match state {
            State::ShutdownOrPoison => {
                panic!("tried reading from shutdown/poisoned stream")
            },
            State::ServerIsWorking { .. } => {
                panic!("tried to write to socket while it should only read from it")
            },
            State::NeedNewAction { waker, buffer } => {
                self.state = self.prepare_next(waker, buffer);
                self.schedule_delayed_wake();
                Ok(Async::NotReady)
            }
            State::ClientIsWorking { expected, waker, mut input } => {
                // first: if !expected.starts_with(input) => assert panic
                expected.assert_same_start(&input);
                // then: if input >= expected { input.advance(expected.len()); state advance too
                let expected_len = expected.len();
                if input.len() >= expected_len {
                    input.advance(expected_len);
                    self.state = State::NeedNewAction { waker, buffer: input };
                    Ok(Async::Ready(()))
                } else {
                    self.state = State::ClientIsWorking { expected, waker, input };
                    Ok(Async::Ready(()))
                }
            }
        }
    }

    fn shutdown(&mut self) -> Poll<(), std_io::Error> {
        // shutdown implies flush, so we flush
        try_ready!(self.poll_flush());
        match &self.state {
            &State::ShutdownOrPoison => (),
            &State::NeedNewAction {..} => (),
            _ => panic!("unexpected state when shutting down")
        }
        self.state = State::ShutdownOrPoison;
        Ok(Async::Ready(()))
    }
}



fn random_amount(max_inclusive: usize) -> usize {
    // max is inclusive but gen_range would make it exclusive
    let max_write = max_inclusive + 1;
    // make it more "likely" to write more stuff
    // (this is statistically horrible hack, but works fine here)
    min(max_inclusive, thread_rng().gen_range(1, max_write + 16))
}

fn write_n_to_slice(from: &BytesMut, to_buf: &mut [u8], n: usize) {
    let copy_to = &mut to_buf[..n];
    let copy_from = &from[..n];
    copy_to.copy_from_slice(copy_from);
}

//TODO potentially add some overlap detection
// i.e. detect if a async read/write/poll was done _before_
// notify was called, which could be done but _should_ not
// be done
fn delayed_waker() -> mpsc::UnboundedSender<Task> {

    let (tx, rx) = mpsc::unbounded();
    thread::spawn(move || {
        let pipe = rx
            .for_each(|task: Task| {
                //sleep some smallish random time
                //sleep between ~ 0ms - 4ms
                let nanos = random::<u32>() / 1000;
                thread::sleep(Duration::new(0, nanos));

                task.notify();
                future::ok::<(),()>(())
            });

        pipe.wait().unwrap()
    });

    tx
}

#[cfg(test)]
mod test {
    #![allow(non_snake_case)]
    use std::time::Duration;
    use std::thread;

    use futures::{future, Future};
    use futures::sync::oneshot;

    fn time_out(secs: u64) -> Box<Future<Item=(), Error=()>> {
        let (tx, rx) = oneshot::channel();
        thread::spawn(move || {
            thread::sleep(Duration::new(secs, 0));
            let _ = tx.send(());
        });

        Box::new(rx.then(|_| future::ok(())))
    }

    mod delayed_waker {
        use futures::Future;

        use super::super::*;
        use super::time_out;

        fn wake_task_later(waker: &Waker) {
            waker.unbounded_send(task::current()).unwrap()
        }

        #[test]
        fn calls_notify() {
            let waker = delayed_waker();

            let mut is_first = true;
            let fut = future::poll_fn(|| -> Poll<(), ()> {
                if is_first {
                    is_first = false;
                    wake_task_later(&waker);
                    Ok(Async::NotReady)
                } else {
                    Ok(Async::Ready(()))
                }
            });

            match fut.select2(time_out(1)).wait() {
                Ok(future::Either::A(_)) => (),
                Ok(future::Either::B(_)) => panic!("time out occured"),
                Err(_e) => unreachable!()
            }
        }
    }

    mod MockSocket {

        use bytes::Bytes;

        use super::super::*;
        use super::time_out;


        #[test]
        fn with_simple_session() {
            use self::ActionData::*;
            use self::Actor::*;

            let mut socket = Some(MockSocket::new(vec![
                (Server, Blob("hy there\r\n".as_bytes().to_owned())),
                (Client, Blob("quit\r\n".as_bytes().to_owned())),
            ]));


            let buf = &mut [0u8, 0, 0, 0] as &mut [u8];
            let mut expect = b"hy there\r\n" as &[u8];

            let fut = future
                ::poll_fn(move || -> Poll<Option<MockSocket>, std_io::Error> {
                    loop {
                        let n = try_ready!(socket.as_mut().unwrap().poll_read(buf));

                        assert!(n > 0);
                        let read = &buf[..n];
                        let (use_expected, new_expected) = expect.split_at(n);
                        expect = new_expected;
                        assert_eq!(use_expected, read);

                        if expect.is_empty() {
                            return Ok(Async::Ready(socket.take()));
                        }
                    }
                })
                .and_then(|mut socket| future::poll_fn(move || {
                    let mut bytes = Bytes::from("quit\r\n");

                    loop {
                        let n = try_ready!(socket.as_mut().unwrap().poll_write(&bytes));

                        assert!(n > 0);
                        bytes.advance(n);
                        if bytes.is_empty() {
                            return Ok(Async::Ready(socket.take()))
                        }
                    }
                }))
                .and_then(|mut socket| future::poll_fn(move || {
                    try_ready!(socket.as_mut().unwrap().shutdown());
                    Ok(Async::Ready(()))
                }))
                .select2(time_out(1));

            match fut.wait() {
                Ok(future::Either::A(_)) => (),
                Ok(future::Either::B(((), _))) => panic!("timeout"),
                Err(_e) => unreachable!()
            }


        }
    }
}