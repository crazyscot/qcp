//! client-side (_initiator_) main loop and supporting structures

mod job;
pub use job::CopyJobSpec;
pub use job::FileSpec;

mod main_loop;
#[allow(clippy::module_name_repetitions)]
pub(crate) use main_loop::client_main;

pub(crate) mod meter;

mod options;
pub use options::Parameters;

pub(crate) mod progress;
pub(crate) use progress::MAX_UPDATE_FPS;

pub(crate) mod ssh;
