#![allow(dead_code)]
//! Smart filter: normalize platform raw JSON into compact filtered payloads.
//!
//! Real submodules: types in [`helpers`], platform pipelines in process / filter_impl /
//! cache_tokens.

mod helpers;
mod process;
mod filter_impl;
mod cache_tokens;

#[cfg(test)]
mod tests_inline;

pub use helpers::{
    ContentAnalysis, DiscordGuildItem, PsnTitleItem, SmartFilter, SmartFilteredData, XboxTitleItem,
};
