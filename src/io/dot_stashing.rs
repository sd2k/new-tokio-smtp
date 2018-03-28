use std::io as std_io;

use futures::{Poll, Future, Async};
use futures::stream::Stream;
use bytes::buf::{Buf, BufMut};

use super::{Io, OUTPUT_BUFFER_INC_SIZE};

impl Io {
    pub fn write_dot_stashed<S>(self, source: S) -> DotStashedWrite<S>
        where S: Stream<Error=std_io::Error>, S::Item: Buf
    {
        DotStashedWrite::new(self, source)
    }
}


#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
enum CrLf {
    None,
    HitCr,
    HitLf
}

pub struct DotStashedWrite<S>
    where S: Stream, S::Item: Buf
{
    io: Option<Io>,
    source: S,
    stash_state: CrLf,
    write_eom_seq: bool
}

impl<S> DotStashedWrite<S>
    where S: Stream<Error=std_io::Error>, S::Item: Buf
{
    fn new(io: Io, source: S) -> Self {
        DotStashedWrite {
            source,
            io: Some(io),
            stash_state: CrLf::None,
            write_eom_seq: false
        }
    }

    fn io_mut(&mut self) -> &mut Io {
        self.io.as_mut().expect("poll after completion")
    }

    fn poll_source(&mut self) -> Poll<Option<S::Item>, std_io::Error> {
        let next = try_ready!(self.source.poll());

        if next.is_none() {
            self.write_eom_seq = true;
            let add_newline = self.stash_state != CrLf::HitLf;
            let need = 3 + if add_newline { 2 } else { 0 };
            let out = self.io_mut().out_buffer(need);
            if add_newline {
                out.put("\r\n");
            }
            out.put(".\r\n");
        }

        Ok(Async::Ready(next))
    }

    fn write_dot_stashed_output(&mut self, unstashed: S::Item) {
        //TODO[buf_management]: scan if unstashed needs stashing and only if so write it, so:
        //  1. have a "default" buffer (maybe mem::replace(&mut bufer.output, Bytes::new())
        //  2. have a alternate buffer which is "just" a S::Item
        //  3. consider using Chain
        //      - but what is with default -> alternate -> default chains
        //        (which are potentially broken)
        //IDEA: have a ChainedRingBuffer chaining slices to a default buffer and already "ready"
        //      buffers into a chain "ring"
        let mut state = self.stash_state;
        {
            let raw_len = unstashed.remaining();
            let out = self.io_mut().out_buffer(raw_len);
            let mut over_capacity = out.remaining_mut() - raw_len;
            for bch in unstashed.iter() {
                let (stash, new_state) = match (bch, state) {
                    (b'\r', CrLf::None) => (false, CrLf::HitCr),
                    (b'\n', CrLf::HitCr) => (false, CrLf::HitLf),
                    (b'.', CrLf::HitLf) => (true, CrLf::None),
                    (_, CrLf::None) => (false, CrLf::None),
                    // this _could_ be invalid data but legacy systems _should_
                    // be able to handle orphan '\r'/'\n' so treat it as ok
                    (_, _) => (false, CrLf::None)
                };
                state = new_state;
                if stash {
                    if over_capacity == 0 {
                        //increase buffer capacity
                        let rem = out.remaining_mut();
                        out.reserve(rem + OUTPUT_BUFFER_INC_SIZE);
                        over_capacity += OUTPUT_BUFFER_INC_SIZE;
                    }
                    over_capacity -= 1;
                    out.put_u8(b'.');
                }
                out.put_u8(bch);
            }
        }
        self.stash_state = state;
    }
}

impl<S> Future for DotStashedWrite<S>
    where S: Stream<Error=std_io::Error>, S::Item: Buf
{
    type Item = Io;
    type Error = std_io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            //TODO the think below is needed so to handle put wrt. buffer capacity (it panics
            // if it rans out of capacity)
            //TODO this can be improved to not flush each slice before dot-stashing the next slice
            // e.g. while buffer has space write dot stashed bytes from self.pending into
            // out buffer while poll_flush is NotReady
            try_ready!(self.io_mut().poll_flush());

            if self.write_eom_seq {
                return Ok(Async::Ready(self.io.take().expect("poll after completion")));
            }

            let pending =
                match try_ready!(self.poll_source()) {
                    Some(p) => p,
                    None => continue
                };

            self.write_dot_stashed_output(pending);
        }
    }
}
