//! What the chat-app services (`channel_group`, `channel_work`,
//! `channel_pairing`) use of her: whether she joins a group's talk, when she
//! sees and answers, what she makes of pictures and stickers, and strangers.
//! Her modules stay inside `services::agent`; anything a channel needs is
//! named here first.

pub mod bits {
    pub(crate) use crate::services::agent::merope::bits::{in_group, picture_again};
}

pub mod doing {
    pub(crate) use crate::services::agent::merope::doing::current;
}

pub mod heard {
    pub(crate) use crate::services::agent::merope::heard::take_in;
}

pub mod joining {
    pub(crate) use crate::services::agent::merope::joining::{Here, Why, decide};
}

pub mod making_sense {
    pub(crate) use crate::services::agent::merope::making_sense::{
        read, remember_told, told_section,
    };
}

pub mod others {
    pub(crate) use crate::services::agent::merope::others::{spoke_up, spoke_up_lately};
}

pub mod seeing {
    pub(crate) use crate::services::agent::merope::seeing::{known, look};
}

pub mod sharing {
    pub(crate) use crate::services::agent::merope::sharing::{Offered, choose};
}

pub mod stickers {
    pub(crate) use crate::services::agent::merope::stickers::{picture, resolve, sent};
}

pub mod strangers {
    pub(crate) use crate::services::agent::merope::strangers::{
        Stranger, adopt, enqueue_after, reply,
    };
}

pub mod timing {
    pub(crate) use crate::services::agent::merope::timing::{
        asleep_now, typing, until_read, where_she_is,
    };
}
