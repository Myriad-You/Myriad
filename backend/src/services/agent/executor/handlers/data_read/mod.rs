//! Agent data-read capability handlers (platform, brew, RSSHub, catalog).

mod brew;
mod brew_generate;
mod catalog;
mod config_time_auth;
mod execute;
mod extras_platform;
mod pages;
mod permission;
mod platform;
mod rsshub;
mod search;

pub use execute::execute;
