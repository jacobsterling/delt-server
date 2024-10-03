// @generated automatically by Diesel CLI.

diesel::table! {
    accounts (account_id) {
        account_id -> Varchar,
        #[max_length = 50]
        user_id -> Varchar,
        last_active -> Nullable<Timestamp>,
        rewards -> Jsonb,
    }
}

diesel::table! {
    games (id) {
        #[max_length = 50]
        id -> Varchar,
        creator -> Varchar,
        config -> Jsonb,
        created_at -> Timestamp,
        ended_at -> Nullable<Timestamp>,
        expiry -> Nullable<Timestamp>,
    }
}

diesel::table! {
    player_sessions (session_id, user_id) {
        session_id -> Uuid,
        #[max_length = 50]
        user_id -> Varchar,
        account_id -> Nullable<Varchar>,
        created_at -> Timestamp,
        ended_at -> Nullable<Timestamp>,
        resolved_at -> Nullable<Timestamp>,
        info -> Nullable<Jsonb>,
    }
}

diesel::table! {
    pools (id) {
        id -> Text,
        result -> Nullable<Jsonb>,
        registered_at -> Timestamp,
        resolved_at -> Nullable<Timestamp>,
    }
}

diesel::table! {
    roles (user_id, role) {
        #[max_length = 50]
        user_id -> Varchar,
        role -> Varchar,
        grant_date -> Timestamp,
    }
}

diesel::table! {
    sessions (id) {
        id -> Uuid,
        game_id -> Varchar,
        pool_id -> Nullable<Text>,
        creator -> Varchar,
        password -> Nullable<Varchar>,
        private -> Bool,
        created_at -> Timestamp,
        started_at -> Nullable<Timestamp>,
        ended_at -> Nullable<Timestamp>,
        last_update -> Nullable<Timestamp>,
        logs -> Jsonb,
        state -> Jsonb,
    }
}

diesel::table! {
    user_sessions (auth_token) {
        auth_token -> Text,
        #[max_length = 50]
        user_id -> Varchar,
        started_at -> Timestamp,
        ended_at -> Nullable<Timestamp>,
    }
}

diesel::table! {
    users (id) {
        #[max_length = 50]
        id -> Varchar,
        password -> Varchar,
        email -> Varchar,
        created_at -> Timestamp,
        last_login -> Nullable<Timestamp>,
        settings -> Jsonb,
    }
}

diesel::table! {
    whitelist (session_id, user_id) {
        session_id -> Uuid,
        #[max_length = 50]
        user_id -> Varchar,
    }
}

diesel::joinable!(accounts -> users (user_id));
diesel::joinable!(games -> users (creator));
diesel::joinable!(player_sessions -> accounts (account_id));
diesel::joinable!(player_sessions -> sessions (session_id));
diesel::joinable!(player_sessions -> users (user_id));
diesel::joinable!(roles -> users (user_id));
diesel::joinable!(sessions -> games (game_id));
diesel::joinable!(sessions -> pools (pool_id));
diesel::joinable!(sessions -> users (creator));
diesel::joinable!(user_sessions -> users (user_id));
diesel::joinable!(whitelist -> sessions (session_id));
diesel::joinable!(whitelist -> users (user_id));

diesel::allow_tables_to_appear_in_same_query!(
    accounts,
    games,
    player_sessions,
    pools,
    roles,
    sessions,
    user_sessions,
    users,
    whitelist,
);
