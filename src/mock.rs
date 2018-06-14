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

/// Represents if the action is taken by `Client` or `Server`
#[derive(Debug)]
pub enum Actor {
    Server,
    Client
}

/// the data send by Client/Server
#[derive(Debug)]
pub enum ActionData {
    /// a number of lines, not containing trailing "\r\n"
    ///
    /// The trailing "\r\n" will be added implicitly
    Lines(Vec<&'static str>),
    /// A blob of bytes
    Blob(Vec<u8>)
}

impl ActionData {

    /// returns the len of the data
    ///
    /// In case of `ActionData::Lines` the implied `"\r\n"` line
    /// endings are added into the length (i.e. len +2 for each line).
    pub fn len(&self) -> usize {
        match *self {
            ActionData::Blob(ref blob) => blob.len(),
            ActionData::Lines(ref lines) => {
                //MAGIC_NUM: +2 = "\r\n".len()
                lines.iter().map(|ln| ln.len() + 2).sum()
            }
        }
    }

    pub fn assert_same_start(&self, other: &[u8]) {

        match *self {
            ActionData::Blob(ref blob) => {
                let use_len = min(blob.len(), other.len());
                let other = &other[..use_len];
                let blob = &blob[..use_len];
                //TODO better error message (assert_eq is a BAD idea here as
                // it will flood the output)
                assert!(blob == other, "unexpected data");
            },
            ActionData::Lines(ref lines) => {
                let mut rem = other;
                for line in lines.iter() {
                    let use_len = min(line.len(), rem.len());
                    let use_of_line = &line[..use_len];
                    let other = &rem[..use_len];
                    //TODO better error message (assert_eq is a BAD idea here as
                    // it will flood the output)
                    assert!(use_of_line.as_bytes() == other, "unexpected data");

                    if use_len < line.len() {
                        // we need more data => brake
                        break;
                    }

                    //check the "\r\n" omitted in Lines
                    rem = check_crlf_start(&rem[use_len..]);
                }
            }
        }
    }
}

fn check_crlf_start(tail: &[u8]) -> &[u8] {
    let mut tail = tail;
    let length = tail.len();
    if length >= 1 {
        assert!(tail[0] == b'\r', "unexpected data, expected '\\r' got {:?} in {}",
            tail[0] as char, String::from_utf8_lossy(tail));
        tail = &tail[1..];
    }
    if length >= 2 {
        assert!(tail[0] == b'\n', "unexpected data, expected '\\n' got {:?}in {}",
            tail[0] as char, String::from_utf8_lossy(tail));
        tail = &tail[1..];
    }

    tail

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
            _ => panic!("trying to schedule wake up on shutdown stream")
        }
    }
}

#[derive(Debug)]
pub struct MockSocket {
    conversation: Vec<(Actor, ActionData)>,
    fake_secure: bool,
    state: State,
    check_shutdown: bool
}

/// MockSocket going through a pre-coded interlocked client-server conversation
///
/// The `client` is the part of the program reading to the socked using `poll_read`
/// and writing using `poll_write`, the server is the mock doing thinks in reserve,
/// i.e. reading when the client writes and writing when the server reads.
///
/// Internally it has following states:
///
/// - `ShutdownOrPoison`, it was shutdown or paniced at some point
/// - `ClientIsWorking`, the client is sending data and the server checks if it is
///   what it expects
/// - `ServerIsWorking`, the server sends back an pre-coded response
/// - `NeedNewAction`, the previous action was completed and a new one is needed
///
impl MockSocket {

    pub fn new(conversation: Vec<(Actor, ActionData)>) -> Self {
        Self::new_with_params(conversation, true)
    }

    pub fn new_no_check_shutdown(conversation: Vec<(Actor, ActionData)>) -> Self {
        Self::new_with_params(conversation, false)
    }

    /// create a new `MockSocket` from a sequence of "actions"
    ///
    /// Actions are taken interlocked between `Client` (client write something, server reads)
    /// and `Server` (server writes something, client reads), which is one of the main
    /// limitations of the Mock implementation.
    pub fn new_with_params(conversation: Vec<(Actor, ActionData)>, check_shutdown: bool) -> Self {
        let mut conversation = conversation;
        //queue => stack
        conversation.reverse();

        MockSocket {
            conversation,
            check_shutdown,
            fake_secure: false,
            state: State::NeedNewAction {
                buffer: BytesMut::new(),
                waker: delayed_waker()
            },
        }
    }

    /// sets the state to `ShutdownOrPoison` and clears the conversation
    pub fn clear(&mut self) {
        self.conversation.clear();
        self.state = State::ShutdownOrPoison;
    }

    fn schedule_delayed_wake(&mut self) {
        self.state.waker()
            .unbounded_send(task::current())
            .unwrap()
    }

    /// has a 1/16 chance to return `NotReady` and schedule the current `Task` to be notified later
    ///
    /// This is used to emulate that the connection is sometimes not ready jet
    /// e.g. because of network latencies. Yes, this makes the tests not 100% deterministic,
    /// but to get them in that direction and still test delays without hand encoding them
    /// would requires using something similar to `quick check`
    pub fn maybe_inject_not_ready(&mut self) -> Poll<(), std_io::Error> {
        // 1/16 chance to be not ready
        if random::<u8>() >= 240 {
            self.schedule_delayed_wake();
            Ok(Async::NotReady)
        } else {
            Ok(Async::Ready(()))
        }
    }

    /// creates the next state for given `waker` and `buffer`
    ///
    /// pop's the next action in the conversation if it's
    /// a `Server` action the returned state will be and
    /// `ServerIsWorking` state and the data of the action
    /// was fully written to the `buffer`. If it's a `Client`
    /// action a `ClientIsWorking` stat is returned.
    ///
    /// # Panics
    ///
    /// - if the conversation is done, i.e. if it is empty
    /// - the next state is a `Server` state and the passed in
    ///   buffer is not empty
    ///
    fn prepare_next(&mut self, waker: Waker, buffer: BytesMut) -> State {
        let (actor, data) = self.conversation.pop()
            .expect("prepare next on empty conversation");

        let mut buffer = buffer;

        match actor {
            Actor::Server => {
                // 1. data into() buffer
                assert!(buffer.is_empty(), "buffer had remaining input: {:?}",
                   String::from_utf8_lossy(buffer.as_ref()));
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

    /// `drop` impl
    ///
    /// # Implementation Detail
    ///
    /// if the thread is not panicking it will panic:
    /// - if the socket was not shutdown
    /// - if the conversation did not end, i.e. was not empty
    fn drop(&mut self) {
        if !thread::panicking() {
            if self.check_shutdown {
                if let State::ShutdownOrPoison = self.state {}
                else { panic!("connection was not shutdown"); }
            }

            assert!(self.conversation.is_empty(), "premature cancellation of conversation");
        }
    }
}

impl From<MockSocket> for ::io::Socket {
    fn from(s: MockSocket) -> Self {
        ::io::Socket::Mock(Box::new(s))
    }
}

impl From<MockSocket> for ::io::Io {
    fn from(s: MockSocket) -> Self {
        let socket: ::io::Socket = s.into();
        ::io::Io::from((socket, ::io::Buffers::new()))
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


// before read/write:
//   read/write --> state == NeedNewAction --> prepare next action
//                                               \-> on no next action
//                                                      |-> on read -> NotReady*
//                                                      \-> on write -> panic
//
// [*]: the client might call read until it read all "ready" data, even if the
//      read data already did contain all data it needs, so we can not panic here
//      through it might life lock the client in other situations, so we need to
//      build in a timeout into all tests
//
// on read:
//   read --> Actor == Server -> part of  buffer into read -> return bytes transmitted
//        |                                 \-> state = NeedNewAction
//        |
//        \-> Actor == Client ->  panic
//
// on write:
//   write --> Actor == Client -> read from buffer -> return ...
//         |                          \-> if "end condition"
//         |                                \-> assert read input == expected input
//         |                                       \-> state = NeedNewAction
//         |
//         \-> Actor == Server -> panic
//
// "end condition"
//    1st: read length >= expected read length
//    2nd: alt condition "\r\n.\r\n" read?
//
// inject NotReady return:
//   on before read
//   on read after transmitting >= 1 byte
//   on before write
//   on write after transmitting >= 1 byte
//
// on NotReady return:
//   send Task to DelayedWakerThread

impl AsyncRead for MockSocket {

    /// `poll_read` impl
    ///
    /// # Implementation Details
    ///
    /// - Can always return with `NotReady` before doing anything.
    /// - panics if the state is `ClientIsWorking` or `ShutdownOrPoison`
    /// - on `NeedNewAction` it advances the state to the next action if
    ///   there is any and returns `NotReady`
    /// - writes a random amount of bytes to the passed in read buffer
    ///   (at last 1), advancing the state to `NeedNewAction` once all bytes
    ///   have been read
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
                if self.conversation.is_empty() {
                    self.state = State::NeedNewAction { waker, buffer };
                } else {
                    self.state = self.prepare_next(waker, buffer);
                }
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

    /// `poll_write` impl
    ///
    /// # Implementation Details
    ///
    /// - Can always return with `NotReady` before doing anything.
    /// - panics if the state is `ServerIsWorking` or `ShutdownOrPoison`
    /// - on `NeedNewAction` it advances the state to the next extion and
    ///   returns `NotReady` panicing if there is no new action
    /// - writes a random amount of passed in bytes (at last 1) to the
    ///   input buffer then returns `Ready` with the written byte count
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


    /// `poll_flush` impl
    ///
    /// # Implementation Details
    ///
    /// - Can always return with `NotReady` before doing anything.
    /// - panics if the state is `ServerIsWorking` or `ShutdownOrPoison`
    /// - on `NeedNewAction` it advances the state to the next action and
    ///   returns `NotReady`, _or_ if there is no further action returns
    ///   `Ready`
    /// - always returns `Ready` in the `ClientIsWorking` state if
    ///   it doesn't panic through a (test) assert
    /// - in `ClientIsWorking` it is asserted that the read buffer and
    ///   expected buffer start the same way (up the the min of the len
    ///   of both). If it is found that all bytes where parsed as expected
    ///   the state is advanced to `NeedNewAction`. If more bytes where
    ///   read then they stay in the buffer which will cause a panic
    ///   when advancing to the next action  if the next action is
    ///   not another `Client` action.
    ///
    ///
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
                //poll flush on NeedNewAction + empty conversation should _not_ panic
                if self.conversation.is_empty() {
                    assert!(buffer.is_empty());
                    Ok(Async::Ready(()))
                } else {
                    self.state = self.prepare_next(waker, buffer);
                    self.schedule_delayed_wake();
                    Ok(Async::NotReady)
                }
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

    /// `shutdown` impl
    ///
    /// # Implementation Details
    ///
    /// - can return `NotReady` in any state
    /// - uses `poll_flush` until everything is flushed
    /// - If state is not `ShutdownOrPoison` or `NeedNewAction` it will panic.
    /// - Be aware that due to the call to flush a state
    ///   of `ClientIsWorking` is likely to change or panic
    ///   in `poll_flush` instead of in `shutdown`
    ///
    ///
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


/// returns a random number in `[1; max_inclusive]`, where` max_inclusive` is the most likely value
///
/// Note: `random_amount(0)` always returns 0, any other value returns a number
/// between 1 and the value (inclusive).
fn random_amount(max_inclusive: usize) -> usize {
    // max is inclusive but gen_range would make it exclusive
    let max_write = max_inclusive + 1;
    // make it more "likely" to write more stuff
    // (this is statistically horrible hack, but works fine here)
    min(max_inclusive, thread_rng().gen_range(1, max_write + 16))
}

/// copies `from[..n]` to `to[..n]`
fn write_n_to_slice(from: &[u8], to: &mut [u8], n: usize) {
    to[..n].copy_from_slice(&from[..n]);
}

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

    mod random_amount {
        use super::super::random_amount;

        #[test]
        fn on_1() {
            for _ in 0..100 {
                assert_eq!(random_amount(1), 1);
            }
        }

        #[test]
        fn on_0() {
            for _ in 0..100 {
                assert_eq!(random_amount(0), 0);
            }
        }

        #[test]
        fn on_X() {
            let x = 10;
            for _ in 0..100 {
                let got = random_amount(x);
                assert!(got >= 1);
                assert!(got <= x);
            }
        }
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

    mod ActionData {
        use std::panic;
        use super::super::ActionData;

        fn should_fail<FN>(func: FN)
            where FN: panic::UnwindSafe + FnOnce()
        {
            match panic::catch_unwind(func) {
                Ok(_) => panic!("closure should have paniced"),
                Err(_) => ()
            }
        }

        #[should_panic]
        #[test]
        fn should_fail_should_panic_on_ok() {
            should_fail(|| ())
        }

        mod len {
            use super::*;

            #[test]
            fn len_blob() {
                let blob = ActionData::Blob("la blob".to_owned().into());
                assert_eq!(blob.len(), 7)
            }

            #[test]
            fn len_lines() {
                let lines = ActionData::Lines(vec![
                    "123",
                    "678"
                ]);

                assert_eq!(lines.len(), 10)
            }

        }

        mod assert_start_same {
            use super::*;


            #[test]
            fn blob_smaller_other() {
                let blob = ActionData::Blob("blob".to_owned().into());
                blob.assert_same_start(b"blo" as &[u8]);
                should_fail(|| blob.assert_same_start(b"blO" as &[u8]));
            }

            #[test]
            fn blob_larger_other() {
                let blob = ActionData::Blob("blob".to_owned().into());
                blob.assert_same_start(b"blob and top" as &[u8]);
                should_fail(|| blob.assert_same_start(b"bloB and top" as &[u8]));
            }

            #[test]
            fn lines_smaller_other() {
                let lines = ActionData::Lines(vec!["123", "678"]);
                lines.assert_same_start(b"123\r\n6" as &[u8]);
                should_fail(|| lines.assert_same_start(b"123\r\n7" as &[u8]));
                should_fail(|| lines.assert_same_start(b"123\n\n6" as &[u8]));
            }

            #[test]
            fn lines_same_len_other() {
                let lines = ActionData::Lines(vec!["123", "678"]);
                lines.assert_same_start(b"123\r\n678\r\n" as &[u8]);
                should_fail(|| lines.assert_same_start(b"123\r\n678\r\r" as &[u8]));

            }

            #[test]
            fn lines_larger_other() {
                let lines = ActionData::Lines(vec!["123", "678"]);
                lines.assert_same_start(b"123\r\n678\r\nho" as &[u8]);
                should_fail(|| lines.assert_same_start(b"123\r\n678\rho" as &[u8]));
            }
        }
    }

    mod MockSocket {

        use bytes::Bytes;

        use super::super::*;
        use super::time_out;

        mod shutdown {
            use super::*;

            #[should_panic]
            #[test]
            fn on_still_working_socket() {
                let waker = delayed_waker();
                let mut socket = MockSocket::new(vec![]);
                socket.state = State::ServerIsWorking {
                    waker, to_be_read: BytesMut::new()
                };

                let _res = future
                    ::poll_fn(|| socket.shutdown())
                    .select(time_out(1)
                        .then(|_| -> Result<(), std_io::Error> { panic!("timeout") }))
                    .wait();
            }

            #[test]
            fn on_done_conversation() {
                let mut socket = MockSocket::new(vec![]);

                let res = future
                    ::poll_fn(|| socket.shutdown())
                    .select(time_out(1)
                        .then(|_| -> Result<(), std_io::Error> { panic!("timeout") }))
                    .wait();

                match res {
                    Ok(_) => (),
                    Err((err, _)) => panic!("unexpected error {:?}", err)
                }
            }

        }



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