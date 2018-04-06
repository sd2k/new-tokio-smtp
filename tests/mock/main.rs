extern crate new_tokio_smtp;
extern crate futures;

//FIXME see if we can put this into Cargo.toml
#[cfg(not(feature="mock_impl"))]
compile_error!("integration tests require \"mock_impl\" feature");

mod command;