//! The host filter for what a client sees on its owner's screen: which
//! window she may read, what is wiped before any model sees it, and where
//! each piece of it may go.
//!
//! Everything can be seen except passwords and secrets; the owner's
//! sensitive matters and other people's words may be seen but are never
//! remembered or sent off the device. Host rules come first, the
//! on-device model second, and anything unsure is treated as sensitive.
//!
//! Pure: no clock, environment, file, network or model call. The client
//! captures the screen, asks its on-device model and shows the owner what
//! she cannot see; these rules decide. The output never carries a wiped or
//! matched value, nor where in the text it was.
//!
//! The pipeline, in order (`decide` combines the steps' results):
//!
//! 1. capture exclusion (password managers) and [`window`] (key files) →
//!    [`Disposition::Exclude`];
//! 2. [`secrets::scan`] and [`identifiers::scan`] wipe text before any
//!    model reads it;
//! 3. an identifier, a sensitive [`sources`] entry, a private window or a
//!    [`text`] rule → [`Disposition::PresentOnly`];
//! 4. model unavailable or language unsupported → [`Disposition::Blind`];
//! 5. other people's words: relayed by the model, checked by [`relay`] →
//!    [`Disposition::WhoWhat`]; the owner's own content: classified by the
//!    model → [`Disposition::Carry`] only when it is clearly not sensitive.

pub mod decide;
pub mod identifiers;
pub mod relay;
pub mod secrets;
pub mod sources;
pub mod text;
pub mod window;

pub use decide::{
    Ask, CLASSIFY_ASKS, Classify, Decided, Decision, Disposition, Leaves, PRESENT_ONLY_LEAVES_KIND,
    Policy, Private, PrivateKind, Reason, Relay, decide, decide_with,
};
pub use identifiers::IdentifierKind;
pub use relay::{RelayCheck, check_relay};
pub use secrets::{SecretKind, Wiped};
pub use sources::{SourceKind, Sources};
pub use text::{TextHit, TextKind, text_hits};
pub use window::{Window, Withheld, window};

/// The shape of every input and output. Bumped when one changes
/// incompatibly; a client names the shape it speaks and a core that does not
/// speak it refuses instead of guessing.
pub const SHAPE: u32 = 1;

/// The rules' revision: changes whenever a list, a pattern or a decision
/// changes, so a client can say which rules put a window where it went.
pub const RULES: &str = "2026-10-20.1";
