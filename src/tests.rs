use std::env;

use rocket::local::Client;
use rocket::http::Status;

use super::rocket;

#[test]
fn test_usage() {
    env::set_var("DATABASE_URL", ":memory:");
    let client = Client::new(rocket()).expect("valid rocket instance");
    let mut response = client.get("/").dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert!(
        response
            .body_string()
            .expect("non-empty body")
            .starts_with("USAGE")
    );
}
