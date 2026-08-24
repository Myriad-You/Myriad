//! Authenticated federation HTTP surface (social graph, rooms, delivery, transfers).
//!
//! Real submodules: [`social`] (most handlers + shared query types) and
//! [`rooms_and_router`] (file transfer handlers + `router()`).

mod rooms_and_router;
mod social;

pub use rooms_and_router::router;
pub use social::{
    admin_federation_domain_move, federation_get_public_room, json_rejection_response,
};
