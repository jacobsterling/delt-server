use crate::types::{Content, GameConfig, GameId, Logs, PlayerInfo, SessionState, UserId};
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::schema;

#[derive(Debug, Clone, Queryable, Serialize, Deserialize, PartialEq)]
#[diesel(table_name = schema::games)]
pub struct Game {
    pub id: GameId,
    pub creator: UserId,
    pub config: GameConfig,
    pub created_at: NaiveDateTime,
    pub ended_at: Option<NaiveDateTime>,
    pub expiry: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize, PartialEq)]
#[diesel(table_name = schema::games)]
pub struct NewGame {
    pub id: GameId,
    pub creator: UserId,
    pub config: GameConfig,
    pub expiry: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Queryable, Insertable, Serialize, Deserialize, PartialEq)]
#[diesel(table_name = schema::pools)]
pub struct PoolRef {
    pub id: String,
    pub result: Option<Content>,
    pub registered_at: NaiveDateTime,
    pub resolved_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize, PartialEq)]
#[diesel(table_name = schema::pools)]
pub struct NewPoolRef {
    pub id: String,
}

#[derive(Debug, Clone, Queryable, Serialize, Deserialize)]
pub struct Role {
    pub user_id: UserId,
    pub role: String,
    pub grant_date: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = schema::roles)]
pub struct NewRole {
    pub user_id: UserId,
    pub role: String,
}

#[derive(Debug, Clone, Queryable, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub password: String,
    pub email: String,
    pub created_at: NaiveDateTime,
    pub last_login: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = schema::users)]
pub struct NewUser {
    pub id: UserId,
    pub password: String,
    pub email: String,
}

#[derive(Debug, Clone, Queryable, Insertable, Serialize, Deserialize, PartialEq)]
#[diesel(table_name = schema::whitelist)]
pub struct Whitelist {
    pub session_id: Uuid,
    pub user_id: UserId,
}

#[derive(Debug, Clone, Queryable, Insertable, Serialize, Deserialize)]
#[diesel(table_name = schema::accounts)]
pub struct Account {
    pub account_id: String,
    pub user_id: UserId,
    pub last_active: Option<NaiveDateTime>,
    pub rewards: Logs,
}

#[derive(Debug, Clone, Queryable, Serialize, Deserialize, PartialEq, QueryableByName)]
#[diesel(table_name = schema::sessions)]
pub struct Session {
    pub id: Uuid,
    pub game_id: GameId,
    pub pool_id: Option<String>,
    pub creator: UserId,
    pub password: Option<String>,
    pub private: bool,
    pub created_at: NaiveDateTime,
    pub started_at: Option<NaiveDateTime>,
    pub ended_at: Option<NaiveDateTime>,
    pub last_update: Option<NaiveDateTime>,
    pub logs: Logs,
    pub state: SessionState,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = schema::sessions)]
pub struct NewSession {
    pub game_id: GameId,
    pub pool_id: Option<String>,
    pub creator: UserId,
    pub password: Option<String>,
    pub private: bool,
    pub state: SessionState,
}

#[derive(Debug, Clone, Queryable, Serialize, Deserialize)]
#[diesel(table_name = schema::user_sessions)]
pub struct UserSession {
    pub auth_token: String,
    pub user_id: UserId,
    pub started_at: NaiveDateTime,
    pub ended_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Queryable, Serialize, Deserialize, Insertable, QueryableByName)]
#[diesel(table_name = schema::player_sessions)]
pub struct PlayerSession {
    pub session_id: Uuid,
    pub user_id: String,
    pub account_id: Option<String>,
    pub created_at: NaiveDateTime,
    pub ended_at: Option<NaiveDateTime>,
    pub resolved_at: Option<NaiveDateTime>,
    pub info: Option<PlayerInfo>,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = schema::player_sessions)]
pub struct NewPlayerSession {
    pub session_id: Uuid,
    pub user_id: String,
    pub info: PlayerInfo,
}
