//! PhotoDesk: an image browser (in the spirit of Adobe Bridge) that launches DarkRaw and LightRaw.
//!
//! The logic lives in small modules with tests; `app` is the egui front end.

pub mod app;
pub mod files;
pub mod launch;
pub mod meta;
pub mod settings;
pub mod theme;
pub mod thumbs;

/// The name shown in the window title bar.
pub const APP_TITLE: &str = "PhotoDesk di Lorenzo D'Apolito";
