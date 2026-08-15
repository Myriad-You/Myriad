//! Configuration API handlers.
//!
//! Core types/settings/save/public_ui live in [`types_build`] (merged from former include! split).
//! Independent surfaces remain real submodules.

mod types_build;
mod platform_test;
mod permissions_oauth;

pub use types_build::*;
pub use platform_test::test_platform;
pub use permissions_oauth::*;
