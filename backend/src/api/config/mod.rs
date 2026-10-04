//! Configuration API handlers.
//!
//! Real submodules (not `include!`) so each file owns its imports and visibility.
//! Former include! split of types/build/backup/save/public_ui is now sibling
//! modules, matching [`crate::api::proxy`].

mod backup;
mod build;
mod extras;
mod flags;
mod permissions_oauth;
mod platform_test;
mod platforms;
mod public_ui;
mod save;
mod secrets;
mod site_icon;
mod types;
mod visibility;

pub use backup::*;
pub use build::*;
pub use extras::*;
#[cfg(test)]
pub(crate) use flags::*;
pub use permissions_oauth::*;
pub use platform_test::test_platform;
pub(crate) use platforms::load_public_platform_summaries;
pub use public_ui::*;
pub use save::*;
pub(crate) use secrets::*;
pub use site_icon::*;
pub use types::*;
pub use visibility::*;
