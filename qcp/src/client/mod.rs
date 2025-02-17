//! client-side (_initiator_) main loop and supporting structures

mod options;
pub use options::Parameters;

mod control;
pub use control::Channel;

mod job;
pub use job::CopyJobSpec;
pub use job::FileSpec;

mod main_loop;
mod meter;
mod progress;
pub mod ssh;

#[allow(clippy::module_name_repetitions)]
pub use main_loop::client_main;

pub use progress::MAX_UPDATE_FPS;
