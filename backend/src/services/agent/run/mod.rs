//! One agent run, whatever channel it came in on: its events as a stream,
//! and the tasks it leaves waiting on the person. HTTP, chat apps and
//! groups all read runs through here.

// What every part of a run reads, as `use super::*`.
use std::sync::Arc;

use axum::{Json, http::StatusCode};
use myriad_error::AppError;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde_json::{Value, json};

use crate::error::HttpError;
use crate::services::agent::AgentProgressEvent;
use crate::services::agent::consciousness::{
    AcceptSource, AutonomyGrantStore, AutonomyVerdict, IntentStatus, IntentStore,
    evaluate_autonomy_grant, intention_may_enter_work,
};
use crate::services::agent::run_hub::create_run;
use crate::services::agent::sessions::persist_assistant_message;

mod boot;
mod envelopes;
mod intentions;
mod request;
mod start;
mod types;
mod waiting;

pub use boot::*;
pub(crate) use envelopes::*;
pub(crate) use intentions::*;
pub(crate) use request::*;
pub(crate) use start::*;
pub use types::*;
pub(crate) use waiting::*;
