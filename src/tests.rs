use std::process::{Command, Stdio};
use std::time::Duration;
use std::thread::sleep;

extern crate serde_json;
extern crate tempfile;

use rocket::local::Client;
use rocket::http::Status;
use self::tempfile::NamedTempFile;

use super::rocket;

const INITIAL_WAIT_DURATION: u64 = 1;
const MAX_WAIT_DURATION: u64 = 32;

macro_rules! run_test {
    (|$client:ident| $block:expr) => ({
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
    })
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
        let error = body["error"].as_str().expect("error is string");
        assert!(error.starts_with("Package not found"), error.to_string());
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

#[test]
fn test_module_stats() {
    run_test!(|client| {
        let response = client.get("/apertium-cat").dispatch();
        assert_eq!(response.status(), Status::Accepted);
        let mut body = parse_response(response);
        assert_eq!(body["name"], "apertium-cat");

        let in_progress = body["in_progress"]
            .as_array_mut()
            .expect("valid in_progress");
        assert_eq!(in_progress.len(), 2);
        in_progress
            .sort_by_key(|entry| entry["kind"].as_str().expect("kind is string").to_string());
        assert_eq!(in_progress[0]["kind"], "Monodix");
        assert_eq!(in_progress[0]["path"], "apertium-cat.cat.dix");
        let revision = in_progress[0]["revision"]
            .as_i64()
            .expect("revision is i64");
        assert!(revision > 500, revision);

        let mut sleep_duration = Duration::from_secs(INITIAL_WAIT_DURATION);
        while sleep_duration < Duration::from_secs(MAX_WAIT_DURATION) {
            let response = client.get("/apertium-cat").dispatch();
            match response.status() {
                Status::TooManyRequests => {
                    println!("Waiting for OK... ({:?})", sleep_duration);
                }
                Status::Ok => {
                    let mut body = parse_response(response);
                    if body["in_progress"]
                        .as_array()
                        .expect("valid in_progress")
                        .is_empty()
                    {
                        assert_eq!(body["name"], "apertium-cat");
                        let mut stats = body["stats"].as_array_mut().expect("valid stats");
                        assert_eq!(stats.len(), 3);
                        stats.sort_by_key(|entry| {
                            entry["path"].as_str().expect("path is string").to_string()
                        });
                        assert_eq!(stats[0]["file_kind"], "Monodix");
                        assert_eq!(stats[0]["stat_kind"], "Entries");
                        assert_eq!(stats[0]["path"], "apertium-cat.cat.dix");
                        assert!(stats[0]["revision"].as_i64().expect("revision is i64") > 500);
                        let value = stats[0]["value"]
                            .as_str()
                            .expect("value is string")
                            .parse::<i32>()
                            .expect("value is i32");
                        assert!(value > 50000, value);
                        return;
                    }
                }
                _ => assert!(false, "recieved invalid status"),
            }

            sleep(sleep_duration);
            sleep_duration *= 2;
        }

        assert!(false, "failed to fetch statistics before timeout");
    });
}
