//! Bounded MCP client runtime. No database, web framework, platform globals,
//! or application credentials: hosts inject only a non-blocking status reporter.
mod actor;
pub mod config;
pub mod manager;
pub mod protocol;
pub mod server;
pub mod transport;

/// Report lifecycle transitions. Implementations must return immediately;
/// asynchronous notification delivery belongs to the embedding application.
pub type StatusReporter = std::sync::Arc<dyn Fn(String, bool) + Send + Sync>;

#[cfg(all(test, unix))]
mod test_support;
