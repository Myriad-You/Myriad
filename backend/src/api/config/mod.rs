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
mod public_ui;
mod save;
mod secrets;
mod types;
mod visibility;

pub use backup::*;
pub use build::*;
pub use extras::*;
pub use flags::*;
pub use permissions_oauth::*;
pub use platform_test::test_platform;
pub use public_ui::*;
pub use save::*;
pub use secrets::*;
pub use types::*;
pub use visibility::*;
