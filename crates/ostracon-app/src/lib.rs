//! The Ostracon desktop app. `state` and `format` hold the UI model and have no GPUI in them, so they are
//! tested headless; the views are thin layers over them.

pub mod format;
pub mod platform;
pub mod state;
