use std::{
    env,
    io::{Error, ErrorKind},
};

use actix_http::HttpMessage;
use actix_web_httpauth::extractors::{
    basic::{BasicAuth, Config},
    AuthenticationError,
};

use actix_web::{self, dev::ServiceRequest};

use diesel::{
    prelude::*,
    r2d2::{ConnectionManager, Pool},
    PgConnection,
};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

use crate::{
    db::models::UserSession,
    handlers::{client::ClientActor, CLIENTS},
};

pub mod models;
pub mod schema;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

lazy_static::lazy_static! {
    pub static ref DB_URL: String = {
        env::var("DATABASE_URL").expect("Error fetching database url")
    };

    pub static ref DB: Pool<ConnectionManager<PgConnection>> = {
      let manager = ConnectionManager::<PgConnection>::new(&*DB_URL);

      Pool::builder()
          .build(manager)
          .expect("Error building a connection pool")
      };
}

pub fn run_migrations() {
    match PgConnection::establish(&*DB_URL)
        .as_mut()
        .expect("Error establishing db connection")
        .run_pending_migrations(MIGRATIONS)
    {
        Ok(_) => println!("Migrations completed."),

        Err(e) => println!("Error running migrations: {}", e),
    }
}

pub async fn validator(
    req: ServiceRequest,
    credentials: BasicAuth,
) -> Result<ServiceRequest, (actix_web::Error, ServiceRequest)> {
    let config = req.app_data::<Config>().cloned().unwrap_or_default();

    use schema::user_sessions::dsl::{auth_token, user_sessions};

    match DB.get().as_mut() {
        Ok(conn) => match user_sessions
            .filter(auth_token.eq(credentials.user_id()))
            .get_result::<UserSession>(conn)
        {
            Ok(UserSession { user_id, .. }) if CLIENTS.lock().unwrap().get(&user_id).is_some() => {
                Err((
                    actix_web::Error::from(Error::new(
                        ErrorKind::AddrInUse,
                        format!("{} already connected", &user_id),
                    )),
                    req,
                ))
            }

            Ok(UserSession { user_id, .. }) => {
                let act = ClientActor::new(user_id);

                req.extensions_mut().insert(act);

                Ok(req)
            }

            Err(e) => Err((AuthenticationError::from(config).into(), req)),
        },

        Err(e) => Err((
            actix_web::Error::from(Error::new(ErrorKind::NotFound, e.to_string())),
            req,
        )),
    }
}
