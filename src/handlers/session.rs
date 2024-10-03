use crate::{
    db::{
        models::{PlayerSession, Session, PoolRef, Game},
        schema, DB,
    },
    handlers:: GLOBAL,
    types::{Content, GameId, Logs, PlayerStats, SessionState, SessionStatus, UserId, GameConfig},
};
use actix::{
    prelude::Actor, ActorContext, AsyncContext, Context, Handler, MessageResult,
};
use chrono::{self, Local, NaiveDateTime};
use diesel::{prelude::*, sql_types::Jsonb, update};
use near_primitives::types::AccountId;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Mutex,
    time::{Duration, Instant},
};
use uuid::Uuid;

use super::{messages::*, ClientInfo, ClientStatus, CLIENTS, SESSIONS};

pub struct SessionActor {
    pub id: Uuid,
    pub game_id: GameId,
    pub host: UserId,
    pub creator: UserId,
    pub clients: Mutex<HashMap<UserId, ClientInfo>>,
    pub pool_id: Option<String>,
    pub resolving: Option<NaiveDateTime>,
    pub state: Mutex<SessionState>,
    pub status: SessionStatus,
    pub started_at: Option<NaiveDateTime>,
    pub duration: Duration,
    pub pause_time: Duration,
    pub paused_at: Option<NaiveDateTime>,
    pub ended_at: Option<NaiveDateTime>,
    pub logger: Logs,
    pub tick: Instant,
}

const TICK_INTERVAL: Duration = Duration::from_millis(1000 / 60);
const LOG_INTERVAL: Duration = Duration::from_secs(10);

impl SessionActor {
    pub fn new(
        Session {
            id,
            game_id,
            state,
            logs,
            pool_id,
            started_at,
            creator,
            ..
        }: Session,
        host: UserId,
    ) -> Self {

        let mut db = DB.get();

        let conn = db.as_mut().unwrap();

        use schema::games::dsl::{games, id as gid};

        let config: GameConfig = games
            .filter(gid.eq(&game_id))
            .get_result::<Game>(conn).unwrap().config;

        Self {
            id,
            game_id,
            host,
            creator,
            clients: Mutex::new(HashMap::new()),
            resolving: None,
            state: Mutex::new(state),
            logger: logs,
            tick: Instant::now(),
            pool_id,
            status: if started_at.is_some() {
                SessionStatus::Standby {
                    paused_at: Local::now().naive_local(),
                    for_duration: None,
                    by: None,
                }
            } else {
                SessionStatus::Starting(None::<Duration>)
            },
            duration: Duration::from_secs_f32(config.duration*60.0),
            pause_time: Duration::default(),
            paused_at: None,
            ended_at: None,
            started_at,
        }
    }

    pub fn log(&self) {
        let mut db = DB.get();

        let conn = db.as_mut().unwrap();

        use schema::sessions::dsl::{id, last_update, logs, sessions, started_at, state};

        let session_state = self.state.lock().unwrap().to_owned();

        update(sessions)
            .filter(id.eq(&self.id))
            .set((
                logs.eq(self.logger.as_sql::<Jsonb>()),
                state.eq(session_state.as_sql::<Jsonb>()),
                last_update.eq(Local::now().naive_local()),
                started_at.eq(self.started_at),
            ))
            .execute(conn)
            .ok();

        let clients = self.clients.lock().unwrap();

        use schema::player_sessions::dsl::{ended_at, info, player_sessions, session_id, user_id};

        for (uid, client_info) in clients.iter() {
            let req = update(player_sessions).filter(
                session_id
                    .eq(&self.id)
                    .and(user_id.eq(uid))
                    .and(ended_at.is_null()),
            );

            match client_info.status {
                ClientStatus::InProgress(_) => {
                    req.set(info.eq(session_state.player_info(&uid, &client_info)))
                        .execute(conn)
                        .ok();
                }

                ClientStatus::Ended(t) => {
                    req.set(ended_at.eq(t)).execute(conn).ok();
                }

                _ => {}
            }
        }
    }

    pub fn toggle_timer(&mut self) {
        match self.paused_at {
            Some(t) => {
                self.pause_time += Local::now()
                    .naive_local()
                    .signed_duration_since(t)
                    .to_std()
                    .unwrap();

                self.paused_at = None;
            }

            None => self.paused_at = Some(Local::now().naive_local()),
        }
    }

    pub fn elapsed(&self) -> Duration {
        return self.started_at.map_or(Duration::default(), |s| {
            Local::now()
                .naive_local()
                .signed_duration_since(s)
                .to_std()
                .unwrap()
        }) - self.pause_time;
    }

    pub fn send_tick(&mut self) {

        let mut clients = self.clients.lock().unwrap();

        let session_state = self.state.lock().unwrap().to_owned();

        let mut actors = Vec::new();

        let mut players = HashMap::new();

        for (id, client_info) in clients.iter_mut() {
            actors.push(client_info.actor.to_owned());
            players.insert(id.to_owned(), session_state.player_info(&id, &client_info));

            match client_info.status {
                ClientStatus::InProgress(mut t) => {
                    t = Local::now()
                        .naive_local()
                        .signed_duration_since(client_info.started_at)
                        .to_std()
                        .unwrap();
                }

                _ => {}
            }
        }
        
        let tick = ServerMessage::Tick {
            players,
            state: session_state.to_owned(),
            tick: Instant::now()
                .duration_since(self.tick.to_owned())
                .as_millis(),
            status: self.status.to_owned(),
        };

        for actor in actors {
            actor.do_send(tick.to_owned());
        }

        self.tick = Instant::now();
    }
}

impl Actor for SessionActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.run_interval(TICK_INTERVAL, |act, ctx| {
            
            match &act.status {
                SessionStatus::Starting(mut t @ None::<Duration> ) => {

                    let mut clients = act.clients.lock().unwrap();

                    if clients.iter().all(|(_, c)| match c.status {
                        ClientStatus::Ready | ClientStatus::Ended(_) => true,
                        _ => false,
                    }) {
                        act.started_at = Some(
                            Local::now()
                                .checked_add_signed(chrono::Duration::seconds(15))
                                .unwrap()
                                .naive_local(),
                        );

                        t = act.started_at.map(|s| {
                            Local::now()
                                .naive_local()
                                .signed_duration_since(s)
                                .to_std()
                                .unwrap()
                        });

                        for (_, client_info) in clients.iter_mut() {
                            client_info.status = ClientStatus::InProgress(
                                Local::now()
                                    .naive_local()
                                    .signed_duration_since(client_info.started_at)
                                    .to_std()
                                    .unwrap(),
                            )
                        }
                    } else {
                        for (id, client_info) in clients.iter_mut() {
                            match client_info.status {
                                ClientStatus::InProgress(_) => {
                                    client_info.status = ClientStatus::Ready
                                }

                                ClientStatus::Loading(t) | ClientStatus::LostConnection(t) => {
                                    if Local::now()
                                        .naive_local()
                                        .signed_duration_since(t)
                                        .num_seconds()
                                        > 60i64
                                    {
                                        ctx.address().do_send(Leave(id.to_owned()))
                                    }
                                }

                                ClientStatus::Ready | ClientStatus::Ended(_) => {}
                            }
                        }
                    }
                }
                SessionStatus::Starting(Some(mut t)) => {
                    if let Some(start_time) = act.started_at {
                        if Local::now().naive_local() > start_time {
                            act.toggle_timer();

                            act.status = SessionStatus::InProgress(act.elapsed())
                        } else {
                            t = Local::now()
                                .naive_local()
                                .signed_duration_since(start_time)
                                .to_std()
                                .unwrap();
                        }
                    }
                }

                SessionStatus::InProgress(mut t) => {
                    if act.elapsed() >= act.duration {
                        act.status = SessionStatus::PostSession;
                    } else {
                        t = act.elapsed();
                    }
                }
                SessionStatus::PostSession => match act.resolving {
                    Some(t)
                        if t.signed_duration_since(Local::now().naive_local())
                            .num_seconds()
                            > 30i64 =>
                    {
                        ctx.address().do_send(SessionEnd);
                    }

                    None => ctx.address().do_send(SessionEnd),

                    _ => {}
                },

                SessionStatus::Standby {
                    paused_at,
                    mut for_duration,
                    by,
                } => {
                    if let Some(duration) = for_duration {
                        if Local::now().naive_local()
                            > paused_at
                                .checked_add_signed(chrono::Duration::from_std(duration).unwrap())
                                .unwrap()
                        {
                            act.status = SessionStatus::InProgress(act.elapsed());

                            act.toggle_timer();
                        }
                    }
                }
            }
            
            act.send_tick();
        });

        ctx.run_interval(LOG_INTERVAL, |act, _| act.log());
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        let mut session_guard = SESSIONS.lock().unwrap();

        session_guard.remove(&self.id);
    }
}

impl Handler<SessionEnd> for SessionActor {
    type Result = ();

    fn handle(&mut self, _: SessionEnd, ctx: &mut Context<Self>) {
        self.toggle_timer();

        let end = self
            .ended_at
            .get_or_insert(Local::now().naive_local())
            .to_owned();

        let mut clients = self.clients.lock().unwrap();

        for (_, client_info) in clients.iter_mut() {
            match client_info.status {
                ClientStatus::Ended(_) => {}

                _ => client_info.status = ClientStatus::Ended(end.to_owned()),
            }
        }

        self.log();

        use schema::sessions::dsl::{ ended_at as session_end, id as session_id, sessions
        };

        let mut db = DB.get();

        let conn = db.as_mut().unwrap();

        self.resolving = Some(Local::now().naive_local());

        match update(sessions)
            .filter(session_id.eq(&self.id))
            .set((
                session_end.eq(end),
            ))
            .execute(conn)
        {
            Ok(_) => {
                let session_state = self.state.lock().unwrap().to_owned();

                use schema::player_sessions::dsl::{player_sessions, session_id};

                match player_sessions.filter(session_id.eq(&self.id)).load::<PlayerSession>(conn) {
                    Ok(res) => {

                    }

                    _ => {}
                }

                match player_sessions
                    .filter(session_id.eq(&self.id))
                    .get_results::<PlayerSession>(conn)
                {
                    Ok(ref mut res) => {
                        let mut result = HashMap::new();

                        for PlayerSession {
                            user_id,
                            account_id,
                            ended_at,
                            resolved_at,
                            ..
                        } in (res as &mut Vec<PlayerSession>).iter_mut() {

                            if let Some(player_session_end) = ended_at {

                                let PlayerStats {
                                    kills,
                                    xp_accrual,
                                    death,
                                } = session_state.stats.get(&user_id as &UserId).unwrap();

                                match account_id {
                                    Some(id) => match AccountId::from_str(&id) {
                                        Ok(id) => {
                                            let xp = match death {
                                                Some(_) => None,
                                                None => {
                                                    result.insert(id.to_owned(), player_session_end.to_owned());
            
                                                    Some(xp_accrual)
                                                }
                                            };
            
                                            if resolved_at.is_none() {
                                                GLOBAL.do_send(PlayerSessionResolve {
                                                    session_id: self.id.to_owned(),
                                                    account_id: id.to_owned(),
                                                    xp: xp.copied(),
                                                });
                                            }
                                        },
                                        Err(_) => {},
                                    },
                                    None => {},
                                };
                            }
                        }

                        if let Some(pool_id) = &self.pool_id {

                            use schema::pools::dsl::{pools, id};

                            match pools
                                .filter(
                                    id.eq(&pool_id)
                                )
                                .get_result::<PoolRef>(conn)
                            {
                                Ok(pool) if pool.resolved_at.is_some() => {
                                    if (res as &mut Vec<PlayerSession>).iter().all(|s| s.resolved_at.is_some()) {
                                        ctx.stop();
                                    }
                                },
                            
                                Err(_) => println!(
                                        "[Server] DB Error During Session Resolve - {}: Unregistered Pool {}",
                                        &self.id,
                                        &pool_id
                                ),
                    
                                Ok(_) => {
                                    let mut results: Vec<(AccountId, NaiveDateTime)> = result
                                        .iter()
                                        .map(|(account_id, player_session_end)| (account_id.to_owned(), player_session_end.to_owned()))
                                        .collect();

                                    results.sort_by(|a, b| b.1.cmp(&a.1));

                                    println!("Results: {:?}", results);

                                    GLOBAL.do_send(SessionResolve {
                                        session_id: self.id.to_owned(),
                                        results,
                                        pool_id: pool_id.to_owned(),
                                    });
                                }
                            }
                        } else {
                            if (res as &mut Vec<PlayerSession>).iter().all(|s| s.resolved_at.is_some()) {
                                ctx.stop();  
                            }
                        }
                    }

                    Err(e) => println!(
                        "[Server] DB Error Ending Session - {} : {}",
                        &self.id,
                        e.to_string()
                    ),
                }
            }

            Err(e) => println!(
                "[Server] DB Error Ending Session - {} : {}",
                &self.id,
                e.to_string()
            ),
        };
    }
}

impl Handler<SessionResolve> for SessionActor {
    type Result = ();

    fn handle(&mut self, SessionResolve { .. }: SessionResolve, ctx: &mut Context<Self>) {
        ctx.stop();
    }
}

impl Handler<SessionMessage> for SessionActor {
    type Result = ();

    fn handle(&mut self, SessionMessage { msg, exclude }: SessionMessage, _: &mut Context<Self>) {
        for (id, client) in self.clients.lock().unwrap().iter() {
            if !exclude.contains(id) {
                client.actor.do_send(msg.to_owned());
            }
        }
        self.logger.log(&msg);
    }
}

impl Handler<Join> for SessionActor {
    type Result = MessageResult<Join>;

    fn handle(
        &mut self,
        Join {
            user_id,
            player_info,
            account_id,
        }: Join,
        ctx: &mut Context<Self>,
    ) -> Self::Result {
        let guard = CLIENTS.lock().unwrap();

        let client_actor = guard.get(&user_id).unwrap();

        let mut clients = self.clients.lock().unwrap();

        clients.insert(
            user_id.to_owned(),
            ClientInfo::new(client_actor.to_owned(), account_id),
        );

        let mut notif = Content::new();

        let msg = format!("{} joined.", &user_id);

        self.logger.log(&msg);

        notif.insert("message", &msg).insert("id", &user_id);

        ctx.notify(SessionMessage {
            msg: ServerMessage::Notification(notif),
            exclude: Vec::new(),
        });

        let mut players = HashMap::new();

        let mut session_state = self.state.lock().unwrap();

        session_state
            .entities
            .set_managed(&player_info.managed_entities, &user_id);

        for (id, client_info) in clients.iter() {
            players.insert(id.to_owned(), session_state.player_info(id, client_info));
        }

        println!("[Server] {:?} has joined {}", &user_id, &self.id);

        MessageResult((session_state.to_owned(), players))
    }
}

impl Handler<Leave> for SessionActor {
    type Result = MessageResult<Leave>;

    fn handle(&mut self, Leave(user_id): Leave, ctx: &mut Context<Self>) -> Self::Result {
        let mut clients = self.clients.lock().unwrap();

        match clients.remove(&user_id) {
            Some(client_info) => {
                let mut notif = Content::new();

                notif
                    .insert("id", &user_id)
                    .insert("message", &format!("{} left.", &user_id));

                let mut session_state = self.state.lock().unwrap();

                let mut managed_entites = session_state.entities.managed(&user_id);

                ctx.notify(SessionMessage {
                    msg: ServerMessage::Left {
                        user_id: user_id.to_owned(),
                        managed_entities: managed_entites.to_owned(),
                    },
                    exclude: vec![user_id.to_owned()],
                });

                println!("[Server] {:?} has left {}", &user_id, self.id.to_owned());

                match clients.iter().next() {
                    Some((new_manager, _)) => {
                        managed_entites = session_state.entities.managed(&user_id);

                        session_state
                            .entities
                            .set_managed(&managed_entites, new_manager);

                        if user_id == self.host {
                            self.host = new_manager.to_owned()
                        }
                    }

                    None => ctx.stop(),
                };

                MessageResult(Some((
                    self.id.to_owned(),
                    session_state.player_info(&user_id, &client_info),
                )))
            }

            None => MessageResult(None),
        }
    }
}

impl Handler<SessionUpdate> for SessionActor {
    type Result = ();

    fn handle(&mut self, SessionUpdate { updater, update }: SessionUpdate, _: &mut Context<Self>) {
        match update {
            Update::Affect {
                affector,
                affected,
                affectors,
            } => {

                let session_state = self.state.lock().unwrap();

                let updater_managed_entities = session_state.entities.managed(&updater);

                if updater_managed_entities.contains(&affector) {

                    let clients = self.clients.lock().unwrap();

                    for (id, ClientInfo { actor, .. }) in
                        clients.iter().filter(|(id, _)| *id != &updater)
                    {
                        let mut affected_entities = HashSet::new();
                        for entity_id in &affected {
                            if session_state.entities.managed(id).contains(entity_id) {
                                affected_entities.insert(entity_id.to_owned());
                            }
                        }
                        actor.do_send(ServerMessage::Update(Update::Affect {
                            affector: affector.to_owned(),
                            affectors: affectors.to_owned(),
                            affected: affected_entities,
                        }))
                    }
                }
            }

            Update::Entities {
                active,
                kill_list,
                spawns,
            } => {

                let mut session_state = self.state.lock().unwrap();

                let updater_managed_entities = session_state.entities.managed(&updater);

                for (id, entity) in active.0.iter() {
                    if updater_managed_entities.contains(id) {
                        session_state
                            .entities
                            .update(id.to_owned(), entity.to_owned());

                        session_state.pending_spawns.remove(id);
                    }
                }

                for id in kill_list.iter() {
                    if updater_managed_entities.contains(id) {
                        if let Some(entity) = session_state.entities.remove(id) {
                            session_state.destroyed_entities.insert(id, entity);
                        }
                    }
                }

                for (id, entity) in spawns.0.iter() {
                    if !session_state.pending_spawns.contains_key(id) {
                        let new_id = session_state.entities.insert(id, entity.to_owned());

                        session_state.pending_spawns.insert(id.to_owned(), new_id);
                    }
                }
            }

            Update::ChangeSpawn(spawn) if updater == self.host => {
                
                let mut session_state = self.state.lock().unwrap();

                session_state.spawn = spawn
            },

            Update::Stats(stats) => {

                let mut session_state = self.state.lock().unwrap();

                session_state.stats.insert(updater.to_owned(), stats);
            }

            Update::Status(status) => 
            {
                let mut clients = self.clients.lock().unwrap();

                let updater_info = clients.get_mut(&updater).unwrap();

                updater_info.status = status
            },

            Update::Pause(None::<Duration>) if updater == self.host => match self.status {
                SessionStatus::InProgress(_) => {
                    self.toggle_timer();

                    self.status = SessionStatus::Standby {
                        paused_at: Local::now().naive_local(),
                        for_duration: None,
                        by: Some(updater.to_owned()),
                    }
                }
                _ => {}
            },

            Update::Pause(for_duration @ Some(_)) => match self.status {
                SessionStatus::InProgress(_) => {
                    self.toggle_timer();

                    self.status = SessionStatus::Standby {
                        paused_at: Local::now().naive_local(),
                        for_duration,
                        by: Some(updater.to_owned()),
                    }
                }
                _ => {}
            },

            Update::Resume => match &self.status {
                SessionStatus::Standby { by, .. }
                    if updater == self.host || by.clone().map(|id| id.to_owned() == updater).unwrap_or(false) =>
                {
                    self.toggle_timer();

                    self.status = SessionStatus::InProgress(self.elapsed());
                }

                _ => {}
            },

            Update::End if updater == self.host => match self.status {
                SessionStatus::InProgress(_) => {
                    self.status = SessionStatus::PostSession;
                }
                _ => {}
            },

            _ => {}
        };

        let mut clients = self.clients.lock().unwrap();

        let updater_info = clients.get_mut(&updater).unwrap();

        updater_info.last_update = Instant::now();
    }
}
