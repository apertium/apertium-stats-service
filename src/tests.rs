use std::process::{Command, Stdio};

extern crate serde_json;
extern crate tempfile;

use self::tempfile::NamedTempFile;
use rocket::http::Status;
use rocket::local::Client;

use super::rocket;

macro_rules! run_test {
    (| $client:ident | $block:expr) => {{
        let db_file = NamedTempFile::new().expect("valid database file");
        let db_path = db_file.path().to_str().expect("valid database path");
        Command::new("diesel")
            .stdout(Stdio::null())
            .args(&["database", "setup"])
            .env("DATABASE_URL", db_path)
            .status()
            .expect("successful database setup");
        let $client = Client::new(rocket(db_path.into())).expect("valid rocket instance");
        $block
    }};
}

fn parse_response(mut response: rocket::local::LocalResponse) -> serde_json::Value {
    serde_json::from_str(&response.body_string().expect("non-empty body"))
        .expect("valid JSON response")
}

#[test]
fn test_usage() {
    run_test!(|client| {
        let mut response = client.get("/").dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = response.body_string().expect("non-empty body");
        assert!(body.starts_with("USAGE"), body);
    });
}

#[test]
fn test_nonexistent_package_stats() {
    run_test!(|client| {
        let response = client.get("/apertium-xxx").dispatch();
        assert_eq!(response.status(), Status::BadRequest);
        let body = parse_response(response);
        assert_eq!(body["name"], "apertium-xxx");
        let error = body["error"].to_string();
        assert!(error.starts_with("\"Package not found"), error);
    });
}

#[test]
fn test_invalid_package_stats() {
    run_test!(|client| {
        let response = client.get("/abcd").dispatch();
        assert_eq!(response.status(), Status::BadRequest);
        let body = parse_response(response);
        assert_eq!(
            body,
            json!({
            "error": "Invalid package name: abcd",
            "name": "abcd"
        })
        );
    });
}
