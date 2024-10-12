//! qcp client main loop

pub mod control;
pub mod job;
mod main_loop;
mod meter;
mod progress;

pub(crate) use main_loop::client_main;
pub(crate) use progress::MAX_UPDATE_FPS;
