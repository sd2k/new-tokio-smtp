use bytes::{BytesMut, BufMut};

// cyclic dep. for double dispatch ergonomics
use ::{Connection, CmdFuture, Cmd, SimpleCmd};


#[macro_export]
macro_rules! impl_simple_command {
    ($(for $name:ident => |$buf:ident| $block:block;)*) => ($(
        impl Cmd for $name {
            #[inline]
            fn exec(self, con: Connection) -> CmdFuture {
                con.simple_cmd(self)
            }
        }

        impl SimpleCmd for $name {
            fn write_cmd(&self, $buf: &mut BytesMut) {
                $block
            }
        }
    )*);
}



pub struct Reset;
pub struct Quit;

impl_simple_command! {
    for Reset => |buf| { buf.put("RSET") };
    for Quit => |buf| { buf.put("QUIT") };
}
