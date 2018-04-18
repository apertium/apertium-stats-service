#[macro_use]
mod common;
mod get;
mod post;

use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

extern crate serde_json;
extern crate tempfile;

use self::tempfile::NamedTempFile;
use rocket::http::Status;
use rocket::local::{Client, LocalResponse};

use super::rocket;

pub const INITIAL_WAIT_DURATION: u64 = 1;
pub const MAX_WAIT_DURATION: u64 = 32;

pub const TEST_LT_MODULE: &str = "cat";
pub const TEST_LT_MODULE_FILES_COUNT: usize = 3;
pub const TEST_LT_MODULE_STATS_COUNT: usize = 4;

pub const TEST_HFST_MODULE: &str = "kaz";
pub const TEST_HFST_MODULE_FILES_COUNT: usize = 5;
pub const TEST_HFST_MODULE_STATS_COUNT: usize = 5;

pub const TEST_PAIR: &str = "kaz-tat";
pub const TEST_PAIR_FILES_COUNT: usize = 7;
pub const TEST_PAIR_STATS_COUNT: usize = 11;

#[test]
fn usage() {
    run_test!(|client| {
        let mut response = client.get("/").dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = response.body_string().expect("non-empty body");
        assert!(body.starts_with("USAGE"), body);
    });
}
