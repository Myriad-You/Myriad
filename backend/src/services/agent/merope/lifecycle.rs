//! What the persona background loop (`crate::persona`) drives: each of her
//! periodic ticks, and stopping her work at shutdown.

pub mod background {
    pub(crate) use crate::services::agent::merope::background::{shutdown, stop_admission};
}

pub mod doing {
    pub(crate) use crate::services::agent::merope::doing::tick;
}

pub mod life {
    pub(crate) use crate::services::agent::merope::life::tick;
}

pub mod memory_jobs {
    pub(crate) use crate::services::agent::merope::memory_jobs::tick;
}

pub mod playing {
    pub(crate) use crate::services::agent::merope::playing::tick;
}

pub mod reach {
    pub(crate) use crate::services::agent::merope::reach::tick;
}

pub mod wander {
    pub(crate) use crate::services::agent::merope::wander::tick;
}
