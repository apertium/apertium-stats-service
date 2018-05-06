use std::ops::Deref;

use diesel::{
    r2d2::{self, ConnectionManager}, sqlite::SqliteConnection,
};
use rocket::{
    http::Status, request::{self, FromRequest}, Outcome, Request, State,
};

pub type Pool = r2d2::Pool<ConnectionManager<SqliteConnection>>;

pub fn init_pool(url: &str) -> Pool {
    let manager = ConnectionManager::<SqliteConnection>::new(url);
    // sqlite cannot handle more than 1 concurrent request
    r2d2::Pool::builder().max_size(1).build(manager).unwrap()
}

pub struct DbConn(pub r2d2::PooledConnection<ConnectionManager<SqliteConnection>>);

impl Deref for DbConn {
    type Target = SqliteConnection;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a, 'r> FromRequest<'a, 'r> for DbConn {
    type Error = ();

    fn from_request(request: &'a Request<'r>) -> request::Outcome<DbConn, ()> {
        let pool = request.guard::<State<Pool>>()?;
        match pool.get() {
            Ok(conn) => Outcome::Success(DbConn(conn)),
            Err(_) => Outcome::Failure((Status::ServiceUnavailable, ())),
        }
    }
}
