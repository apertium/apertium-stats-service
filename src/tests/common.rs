use super::*;

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

pub fn parse_response(mut response: LocalResponse) -> serde_json::Value {
    serde_json::from_str(&response.body_string().expect("non-empty body"))
        .expect("valid JSON response")
}

pub fn parse_i32_value(value: &serde_json::Value) -> i32 {
    return value
        .as_str()
        .expect("value is string")
        .parse::<i32>()
        .expect("value is i32");
}

pub fn wait_for_ok<F>(client: &Client, endpoint: &str, handle_ok_response: F)
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