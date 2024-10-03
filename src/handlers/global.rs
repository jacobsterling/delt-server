use actix::{Actor, ActorFutureExt, AsyncContext, Context, Handler, WrapFuture};
use chrono::Local;
use delt_d::staking::Pool;
use diesel::{prelude::*, update};
use near_primitives::types::AccountId;

use std::{
    str::FromStr,
    time::{Duration, Instant},
};

use crate::{
    db::{
        models::{PlayerSession, PoolRef, Session},
        schema, DB,
    },
    handlers::{messages::SessionEnd, session::SessionActor},
    types::UserId,
};

use super::{
    contract_methods::{assert_pool_result, distribute_stakes, get_pools, give_xp, kill_character},
    messages::{PlayerSessionResolve, ServerError, SessionResolve},
    SESSIONS,
};

const GLOBAL_TICK_INTERVAL: Duration = Duration::from_millis(1000 / 60);
pub struct GlobalActor {
    tick: Instant,
}

impl Default for GlobalActor {
    fn default() -> Self {
        Self {
            tick: Instant::now(),
        }
    }
}

impl Actor for GlobalActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        use schema::player_sessions::dsl::{player_sessions, resolved_at};
        use schema::pools::dsl::{pools, resolved_at as pool_resolved_at};
        use schema::sessions::dsl::{ended_at, sessions};

        let mut db = DB.get();

        let conn = db.as_mut().unwrap();

        let mut guard = SESSIONS.lock().unwrap();

        match sessions
            .inner_join(pools)
            .inner_join(player_sessions)
            .filter(
                ended_at
                    .is_not_null()
                    .and(pool_resolved_at.is_null().or(resolved_at.is_null())),
            )
            .get_results::<(Session, PoolRef, PlayerSession)>(conn)
        {
            Ok(s) => {
                for (session, _, _) in s {
                    let session_actor = guard.entry(session.id.to_owned()).or_insert(
                        SessionActor::new(session.to_owned(), session.creator.to_owned()).start(),
                    );

                    session_actor.do_send(SessionEnd);
                }
            }

            Err(_) => {}
        }

        ctx.run_interval(GLOBAL_TICK_INTERVAL, |act, _ctx| {
            act.tick = Instant::now();
        });
    }
}

impl Handler<SessionResolve> for GlobalActor {
    type Result = ();

    fn handle(
        &mut self,
        SessionResolve {
            results,
            pool_id,
            session_id,
        }: SessionResolve,
        ctx: &mut Self::Context,
    ) {
        let pid = pool_id.to_owned();

        ctx.spawn(
            async move {
                match get_pools(None).await {
                    Ok(all_pools) => match all_pools.get(&pool_id) {
                        Some(Pool {
                            required_stakes,
                            result,
                            ..
                        }) if result.is_none() => {
                            let mut final_result = None;

                            for (aid, _) in results.iter() {
                                for (res, stakes) in required_stakes.0.iter() {
                                    let pool_result = AccountId::from_str(res.as_str()).unwrap();

                                    if aid == &pool_result {
                                        final_result = Some(pool_result);
                                        break;
                                    } else {
                                        for (account_id, _) in stakes.iter() {
                                            if account_id.as_str() == aid.as_str() {
                                                final_result = Some(pool_result);

                                                break;
                                            }
                                        }
                                    }
                                }
                            }

                            match assert_pool_result(pool_id.to_owned(), final_result).await {
                                Ok(_) => distribute_stakes(pool_id.to_owned()).await,

                                Err(e) => Err(e),
                            }
                        }

                        Some(_) => Err(ServerError::Transaction(
                            format!("Pool has already been resolved {}", &pool_id).to_string(),
                        )),

                        None => Err(ServerError::Query(
                            format!("Pool has been removed {}", &pool_id).to_string(),
                        )),
                    },

                    Err(e) => Err(e),
                }
            }
            .into_actor(self)
            .map(move |res, _act, _ctx| match res {
                Ok(_) => {
                    let mut db = DB.get();

                    let conn = db.as_mut().unwrap();

                    use schema::pools::dsl::{id, pools, resolved_at};

                    update(pools)
                        .filter(id.eq(&pid))
                        .set(resolved_at.eq(Local::now().naive_local()))
                        .execute(conn)
                        .ok();
                }

                Err(e) => println!(
                    "[Server] RPC Error During Session End - {}: {}",
                    &session_id,
                    e.to_string()
                ),
            }),
        );
    }
}

impl Handler<PlayerSessionResolve> for GlobalActor {
    type Result = ();

    fn handle(
        &mut self,
        PlayerSessionResolve {
            session_id,
            account_id,
            xp,
        }: PlayerSessionResolve,
        ctx: &mut Self::Context,
    ) {
        use schema::player_sessions::dsl::{
            account_id as aid, player_sessions, resolved_at, session_id as id,
        };

        let mut db = DB.get();

        let conn = db.as_mut().unwrap();

        match player_sessions
            .filter(
                aid.eq(account_id.as_str())
                    .and(id.eq(&session_id))
                    .and(resolved_at.is_null()),
            )
            .get_result::<PlayerSession>(conn)
        {
            Ok(_) => {
                ctx.spawn(
                    async move {
                        match xp {
                            Some(xp) => match give_xp(&account_id, &xp).await {
                                Ok(_) => Ok(account_id),
                                Err(e) => Err(e),
                            },

                            None => match kill_character(&account_id).await {
                                Ok(_) => Ok(account_id),
                                Err(e) => Err(e),
                            },
                        }
                    }
                    .into_actor(self)
                    .map(move |res, _act, _ctx| match res {
                        Ok(account_id) => {
                            let mut db = DB.get();

                            let conn = db.as_mut().unwrap();

                            use schema::accounts::dsl::{account_id as aid, accounts, user_id};

                            match accounts
                                .filter(aid.eq(account_id.to_string()))
                                .select(user_id)
                                .get_result::<UserId>(conn)
                            {
                                Ok(uid) => {
                                    use schema::player_sessions::dsl::user_id;

                                    match update(player_sessions)
                                        .filter(id.eq(&session_id).and(user_id.eq(&uid)))
                                        .set(resolved_at.eq(Local::now().naive_local()))
                                        .execute(conn)
                                    {
                                        Ok(_) => {}

                                        Err(e) => println!("[Server] DB Error: {}", e.to_string()),
                                    }
                                }

                                Err(e) => println!("[Server] Internal Error: {}", e.to_string()),
                            }
                        }
                        Err(e) => println!("[Server] RPC Error: {}", e.to_string()),
                    }),
                );
            }

            Err(_) => {}
        };
    }
}
