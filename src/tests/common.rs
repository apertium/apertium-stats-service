use rocket_contrib::json::JsonValue;

use super::*;

lazy_static! {
    pub static ref PACKAGE_LISTING_JSONL: &'static str = include_str!("fixtures/package_listing.jsonl");
    pub static ref PACKAGE_LISTING: Vec<JsonValue> = PACKAGE_LISTING_JSONL
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
}

pub fn setup_database() -> tempfile::NamedTempFile {
    let db_file = NamedTempFile::new().expect("valid database file");
    {
        let db_path = db_file.path().to_str().expect("valid database path");
        Command::new("diesel")
            .stdout(Stdio::null())
            .args(&["database", "setup"])
            .env("DATABASE_URL", db_path)
            .status()
            .expect("successful database setup");
    }

    db_file
}

macro_rules! run_test {
    (| $client:ident | $block:expr) => {{
        let db_file = $crate::tests::common::setup_database();
        let db_path = db_file.path().to_str().expect("valid database path");
        let $client = Client::new(service(db_path.into(), None, None)).expect("valid rocket instance");
        $block
    }};
}

macro_rules! run_test_with_github_auth {
    (| $client:ident | $block:expr) => {{
        use httpmock::{Method::POST, MockServer};

        let github_auth_token = "fake_token";

        let db_file = $crate::tests::common::setup_database();
        let db_path = db_file.path().to_str().expect("valid database path");

        let server = MockServer::start();
        for (i, listing) in PACKAGE_LISTING.iter().enumerate() {
            let after = if i == 0 {
                json!(null)
            } else {
                JsonValue(PACKAGE_LISTING[i-1]["data"]["organization"]["repositories"]["pageInfo"]["endCursor"].clone())
            };

            println!("listing {} after is {:?}", i, after);
            server.mock(|when, then| {
                when.method(POST)
                    .path("/")
                    .header("Authorization", &format!("Bearer {}", github_auth_token))
                    .header("Content-Type", "application/json")
                    .json_body_partial(json!({
                        "operationName": "PackagesQuery",
                        "variables": {"after": after},
                    }).to_string());
                then
                    .status(200)
                    .header("Content-Type", "application/json")
                    .body(serde_json::to_string(listing).unwrap());
            });
        }

        let $client = Client::new(service(
            db_path.into(),
            Some(&github_auth_token),
            Some(&server.base_url()),
        ))
        .expect("valid rocket instance");
        $block
    }};
}

pub fn parse_response(mut response: LocalResponse) -> JsonValue {
    JsonValue(serde_json::from_str(&response.body_string().expect("non-empty body")).expect("valid JSON response"))
}

pub fn wait_for_ok<F>(client: &Client, endpoint: &str, handle_ok_response: F)
where
    F: Fn(LocalResponse) -> bool,
{
    let mut sleep_duration = INITIAL_WAIT_DURATION;
    while sleep_duration < MAX_WAIT_DURATION {
        let response = client.get(endpoint).dispatch();
        match response.status() {
            Status::TooManyRequests => {
                println!("Waiting for OK... ({:?})", sleep_duration);
            },
            Status::Ok => {
                if handle_ok_response(response) {
                    return;
                }
            },
            status => panic!("recieved unexpected status: {:?}", status),
        }

        sleep(sleep_duration);
        sleep_duration *= 2;
    }

    panic!("failed while waiting for completion");
}
