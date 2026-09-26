//! One agent run, whatever channel it came in on: its events as a stream,
//! and the tasks it leaves waiting on the person. HTTP, chat apps and
//! groups all read runs through here.

mod envelopes;
mod waiting;

pub(crate) use envelopes::*;
pub(crate) use waiting::*;
