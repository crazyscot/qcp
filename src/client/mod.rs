//! client-side (_initiator_) main loop and supporting structures

pub mod args;
pub mod control;
pub mod job;
mod main_loop;
mod meter;
mod progress;

#[allow(clippy::module_name_repetitions)]
pub use main_loop::client_main;

pub use progress::MAX_UPDATE_FPS;
