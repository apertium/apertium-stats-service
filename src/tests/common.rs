use std::sync::Mutex;

use rocket_contrib::json::JsonValue;
use serde_json;

use super::*;

lazy_static! {
    pub static ref GITHUB_AUTH_MUTEX: Mutex<()> = Mutex::new(());
}

pub fn setup_database() -> Box<tempfile::NamedTempFile> {
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

    Box::new(db_file)
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
        use std::{panic, sync, thread};

        dotenv().ok();
        let db_file = $crate::tests::common::setup_database();
        let db_path = db_file.path().to_str().expect("valid database path");
        let github_auth_token =
            Some(env::var("GITHUB_AUTH_TOKEN").expect("testing requires GITHUB_AUTH_TOKEN environment variable"));
        let guard = GITHUB_AUTH_MUTEX.lock().unwrap();
        if let Err(err) = panic::catch_unwind(|| {
            let termination_signal = sync::Arc::new(sync::atomic::AtomicBool::new(false));
            let $client = Client::new(service(db_path.into(), github_auth_token, Some(termination_signal.clone())))
                .expect("valid rocket instance");
            $block
            termination_signal.store(true, sync::atomic::Ordering::SeqCst);
            thread::sleep(Duration::from_secs(10));
        }) {
            drop(guard);
            panic::resume_unwind(err);
        }
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
