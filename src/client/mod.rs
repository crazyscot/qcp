//! qcp client main loop

pub mod args;
pub mod control;
pub mod job;
mod main_loop;
mod meter;
mod progress;

#[allow(clippy::module_name_repetitions)]
pub(crate) use main_loop::client_main;

pub use progress::MAX_UPDATE_FPS;
