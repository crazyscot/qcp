//! qcp client main loop

pub mod control;
pub mod job;
mod main_loop;
mod meter;

pub(crate) use main_loop::client_main;
