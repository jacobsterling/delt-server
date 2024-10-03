use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use actix::{Actor, Addr};
use chrono::{Local, NaiveDateTime};
use near_primitives::types::AccountId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    handlers::{client::ClientActor, global::GlobalActor, session::SessionActor},
    types::UserId,
};

pub mod client;
pub mod contract_methods;
pub mod global;
pub mod messages;
pub mod session;

lazy_static::lazy_static! {
    pub static ref CLIENTS: Mutex<HashMap<UserId, Addr<ClientActor>>> = Mutex::new(HashMap::new());

    pub static ref SESSIONS: Mutex<HashMap<Uuid, Addr<SessionActor>>> = Mutex::new(HashMap::new());

    pub static ref GLOBAL: Addr<GlobalActor> = GlobalActor::default().start();
}

pub struct ClientInfo {
    pub started_at: NaiveDateTime,
    pub last_update: Instant,
    pub ms: Vec<u32>,
    pub actor: Addr<ClientActor>,
    pub account_id: Option<AccountId>,
    pub status: ClientStatus,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ClientStatus {
    Loading(NaiveDateTime),
    LostConnection(NaiveDateTime),
    InProgress(Duration),
    Ready,
    Ended(NaiveDateTime),
}

impl ClientInfo {
    pub fn new(actor: Addr<ClientActor>, account_id: Option<AccountId>) -> Self {
        Self {
            started_at: Local::now().naive_local(),
            last_update: Instant::now(),
            ms: Vec::new(),
            actor,
            account_id,
            status: ClientStatus::Loading(Local::now().naive_local()),
        }
    }
}
