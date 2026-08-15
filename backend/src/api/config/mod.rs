//! Configuration API handlers.
//!
//! Core types/settings/save/public_ui live in [`types_build`] (merged from former include! split).
//! Independent surfaces remain real submodules.

mod permissions_oauth;
mod platform_test;
mod types_build;

pub use permissions_oauth::*;
pub use platform_test::test_platform;
pub use types_build::*;
