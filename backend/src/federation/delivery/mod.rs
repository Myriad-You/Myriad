//! Outbound federation delivery queue, user observability APIs, and dispatch helpers.
//!
//! Real modules: [`queue`], [`query`], [`dispatch`]. Classification/retry unit
//! tests live in [`dispatch_helpers`].

mod dispatch;
mod query;
mod queue;

pub use dispatch::*;
pub use query::*;
pub use queue::*;

#[cfg(test)]
mod dispatch_helpers;
