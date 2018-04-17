use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

extern crate serde_json;
extern crate tempfile;

use self::tempfile::NamedTempFile;
use rocket::http::Status;
use rocket::local::{Client, LocalResponse};

use super::rocket;

const INITIAL_WAIT_DURATION: u64 = 1;
const MAX_WAIT_DURATION: u64 = 32;

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

fn parse_response(mut response: LocalResponse) -> serde_json::Value {
    serde_json::from_str(&response.body_string().expect("non-empty body"))
        .expect("valid JSON response")
}

fn parse_i32_value(value: &serde_json::Value) -> i32 {
    return value
        .as_str()
        .expect("value is string")
        .parse::<i32>()
        .expect("value is i32");
}

fn wait_for_ok<F>(client: &Client, endpoint: &str, handle_ok_response: F)
where
    F: Fn(LocalResponse) -> bool,
{
    let mut sleep_duration = Duration::from_secs(INITIAL_WAIT_DURATION);
    while sleep_duration < Duration::from_secs(MAX_WAIT_DURATION) {
        let response = client.get(endpoint.clone()).dispatch();
        match response.status() {
            Status::TooManyRequests => {
                println!("Waiting for OK... ({:?})", sleep_duration);
            }
            Status::Ok => {
                if handle_ok_response(response) {
                    return;
                }
            }
            status => assert!(false, format!("recieved unexpected status: {:?}", status)),
        }

        sleep(sleep_duration);
        sleep_duration *= 2;
    }

    assert!(false, "failed while waiting for completion");
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
fn test_get_module_stats() {
    let lang = "kaz";
    let module = format!("apertium-{}", lang);
    let endpoint = format!("/{}", module);

    run_test!(|client| {
        let response = client.get(endpoint.clone()).dispatch();
        assert_eq!(response.status(), Status::Accepted);
        let mut body = parse_response(response);
        assert_eq!(body["name"], module);

        let in_progress = body["in_progress"]
            .as_array_mut()
            .expect("valid in_progress");
        assert_eq!(in_progress.len(), 5);
        in_progress
            .sort_by_key(|entry| entry["path"].as_str().expect("path is string").to_string());
        assert_eq!(in_progress[0]["kind"], "Twol");
        assert_eq!(
            in_progress[0]["path"],
            format!("apertium-{0}.err.twol", lang)
        );
        let revision = in_progress[0]["revision"]
            .as_i64()
            .expect("revision is i64");
        assert!(revision > 500, revision);

        wait_for_ok(&client, &endpoint, |response| {
            let mut body = parse_response(response);
            if body["in_progress"]
                .as_array()
                .expect("valid in_progress")
                .is_empty()
            {
                assert_eq!(body["name"], module);
                let stats = body["stats"].as_array_mut().expect("valid stats");
                assert_eq!(stats.len(), 5);
                stats.sort_by_key(|entry| {
                    entry["path"].as_str().expect("path is string").to_string()
                });
                assert_eq!(stats[0]["file_kind"], "Twol");
                assert_eq!(stats[0]["stat_kind"], "Rules");
                assert_eq!(stats[0]["path"], format!("apertium-{0}.err.twol", lang));
                let revision = stats[0]["revision"].as_i64().expect("revision is i64");
                assert!(revision > 500, revision);
                let value = parse_i32_value(&stats[0]["value"]);
                assert!(value > 15, value);

                let response = client.get(endpoint.clone()).dispatch();
                assert_eq!(response.status(), Status::Ok);
                let body = parse_response(response);
                assert_eq!(body["name"], module);
                assert!(
                    body["in_progress"]
                        .as_array()
                        .expect("valid in_progress")
                        .is_empty(),
                    body["in_progress"].to_string()
                );
                assert_eq!(body["stats"].as_array().expect("valid stats").len(), 5);

                return true;
            }

            return false;
        });
    });
}

#[test]
fn test_get_pair_stats() {
    let lang = "kaz-tat";
    let module = format!("apertium-{}", lang);
    let endpoint = format!("/{}", module);

    run_test!(|client| {
        let response = client.get(endpoint.clone()).dispatch();
        assert_eq!(response.status(), Status::Accepted);
        let mut body = parse_response(response);
        let in_progress = body["in_progress"]
            .as_array_mut()
            .expect("valid in_progress");
        assert_eq!(in_progress.len(), 7);

        wait_for_ok(&client, &endpoint, |response| {
            let mut body = parse_response(response);
            if body["in_progress"]
                .as_array()
                .expect("valid in_progress")
                .is_empty()
            {
                assert_eq!(body["name"], module);
                let stats = body["stats"].as_array().expect("valid stats");
                assert_eq!(stats.len(), 11);
                assert!(
                    stats
                        .iter()
                        .map(|entry| (
                            entry["stat_kind"].as_str().expect("kind is string"),
                            parse_i32_value(&entry["value"]),
                        ))
                        .all(|(kind, value)| kind == "Macros" || value > 0),
                    body["stats"].to_string(),
                );

                return true;
            }

            return false;
        });
    });
}

#[test]
fn test_get_dix_module_stats() {
    let lang = "cat";
    let module = format!("apertium-{}", lang);
    let endpoint = format!("/{}/monodix", module);

    run_test!(|client| {
        let response = client.get(endpoint.clone()).dispatch();
        assert_eq!(response.status(), Status::Accepted);
        let mut body = parse_response(response);
        let in_progress = body["in_progress"]
            .as_array_mut()
            .expect("valid in_progress");
        assert_eq!(in_progress.len(), 1);

        wait_for_ok(&client, &endpoint, |response| {
            let mut body = parse_response(response);
            if body["in_progress"]
                .as_array()
                .expect("valid in_progress")
                .is_empty()
            {
                assert_eq!(body["name"], module);
                let stats = body["stats"].as_array().expect("valid stats");
                assert_eq!(stats.len(), 2);
                assert_eq!(stats[0]["path"], format!("apertium-{0}.{0}.dix", lang));
                assert_eq!(
                    stats[0]["revision"].as_i64().expect("revision1 is i64"),
                    stats[0]["revision"].as_i64().expect("revision2 is i64")
                );
                let value1 = parse_i32_value(&stats[0]["value"]);
                assert!(value1 > 500, value1);
                let value2 = parse_i32_value(&stats[1]["value"]);
                assert!(value2 > 500, value2);

                let response = client.get(endpoint.clone()).dispatch();
                assert_eq!(response.status(), Status::Ok);
                let mut body = parse_response(response);
                assert_eq!(body["name"], module);
                assert!(
                    body["in_progress"]
                        .as_array()
                        .expect("valid in_progress")
                        .is_empty(),
                    body["in_progress"].to_string()
                );
                assert_eq!(body["stats"].as_array().expect("valid stats").len(), 2);

                return true;
            }

            return false;
        });
    });
}

#[test]
fn test_post_package_stats() {
    let lang = "cat";
    let module = format!("apertium-{}", lang);
    let endpoint = format!("/{}/monodix", module);

    run_test!(|client| {
        let response = client.get(endpoint.clone()).dispatch();
        assert_eq!(response.status(), Status::Accepted);

        wait_for_ok(&client, &endpoint, |response| {
            let mut body = parse_response(response);
            if body["in_progress"]
                .as_array()
                .expect("valid in_progress")
                .is_empty()
            {
                let mut stats = body["stats"].as_array_mut().expect("valid stats");
                stats.sort_by_key(|entry| {
                    entry["stat_kind"]
                        .as_str()
                        .expect("stat_kind is string")
                        .to_string()
                });
                let created = stats[0]["created"].as_str().expect("created is string");

                let response = client.post(endpoint.clone()).dispatch();
                assert_eq!(response.status(), Status::Accepted);
                let mut body = parse_response(response);
                let in_progress = body["in_progress"]
                    .as_array_mut()
                    .expect("valid in_progress");
                assert_eq!(in_progress.len(), 1);

                wait_for_ok(&client, &endpoint, |response| {
                    let mut body = parse_response(response);
                    if body["in_progress"]
                        .as_array()
                        .expect("valid in_progress")
                        .is_empty()
                    {
                        assert_eq!(body["name"], module);
                        let mut new_stats = body["stats"].as_array_mut().expect("valid stats");
                        new_stats.sort_by_key(|entry| {
                            entry["stat_kind"]
                                .as_str()
                                .expect("stat_kind is string")
                                .to_string()
                        });
                        let new_created =
                            new_stats[0]["created"].as_str().expect("created is string");
                        assert!(new_created > created);

                        return true;
                    }

                    return false;
                });

                return true;
            }

            return false;
        });
    });
}
