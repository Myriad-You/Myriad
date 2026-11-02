//! Handing work off from chat: an IM private chat, and the web panel.
//!
//! In her conversation she talks as herself (Chat). When they ask
//! her to get something done, she hands it off: she says so, and puts one
//! `[[work: …]]` line at the end with what to do. The channel then starts an
//! ordinary Work run for them with that instruction, as if they had asked for
//! it themselves: the same granted permissions, the same confirmations and
//! questions, delivered the same way. Her own ideas she asks about first.
//!
//! Each thing she hands off is a piece of its own, numbered in the chat
//! (`#1`, `#2`, …), and several can be under way at once. What they say
//! later for one of them she passes on with `[[work #N: …]]`: the answer to
//! its question if it is waiting for one, else a new instruction into it,
//! nothing started over (a piece that is done picks up again in its own
//! session). `[[stop #N]]` stops that one alone; a stopped one stays stopped.
//!
//! The next time they talk she sees every piece: what it is, how far it got,
//! whether it waits for them. A channel sets this up
//! (`RequestContext::channel_chat`) and the web panel its own
//! (`RequestContext::panel_work`, see `panel_work`); both read the same
//! `section`. Reading her lines and the section's words are the shared
//! core's (`myriad_merope::handoff`), the same on every end.

use myriad_merope::handoff::Host;
pub use myriad_merope::handoff::{HandOff, split};

use crate::services::agent::types::ChannelChat;

/// What she knows about handing work off, for a private chat on this site.
pub fn section(chat: &ChannelChat) -> String {
    myriad_merope::handoff::section(Host::Site, &chat.works, chat.waiting, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn she_sees_the_pieces_the_channel_lists() {
        let quiet = section(&ChannelChat::default());
        assert!(quiet.contains("working with their data on this site"));
        assert!(!quiet.contains("handed_off"));
        let busy = section(&ChannelChat {
            works: vec![
                "#2「订餐厅」在等对方回答：要几点？".into(),
                "#3「订票」在等对方回答：哪天？".into(),
            ],
            waiting: 2,
            ..Default::default()
        });
        assert!(busy.contains("#2「订餐厅」"));
        assert!(busy.contains("2 pieces are waiting for their answer"));
    }
}
