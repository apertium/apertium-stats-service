#![feature(plugin, custom_attribute, try_trait)]
#![plugin(rocket_codegen)]
#![deny(clippy)]
#![allow(needless_pass_by_value)]

mod db;
mod models;
mod schema;
mod stats;
mod util;
mod worker;

extern crate chrono;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_derive_enum;
extern crate dotenv;
#[macro_use]
extern crate lazy_static;
extern crate regex;
extern crate rocket;
#[macro_use]
extern crate rocket_contrib;
extern crate rocket_cors;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

use std::env;

use diesel::dsl::sql;
use dotenv::dotenv;
use rocket_contrib::Json;
use rocket_cors::{AllowedHeaders, AllowedOrigins};
use rocket::http::{Method, Status};
use rocket::State;
use self::diesel::prelude::*;

use db::DbConn;
use schema::entries as entries_db;
use worker::Worker;
use util::{normalize_name, JsonResult};
use models::FileKind;

pub const ORGANIZATION_ROOT: &str = "https://github.com/apertium";
pub const ORGANIZATION_RAW_ROOT: &str = "https://raw.githubusercontent.com/apertium";
pub const LANG_CODE_RE: &str = r"\w{2,3}(_\w+)?";

#[get("/")]
fn index() -> &'static str {
    "USAGE

GET /apertium-<code1>(-<code2>)
retrieves statistics for the specified package

GET /apertium-<code1>(-<code2>)/<kind>
retrieves <kind> statistics for the specified package

POST /apertium-<code1>(-<code2>)
calculates statistics for the specified package

POST /apertium-<code1>(-<code2>)/<kind>
calculates <kind> statistics for the specified package"
}

#[get("/<name>")]
fn get_stats(name: String, conn: DbConn, worker: State<Worker>) -> JsonResult {
    let normalized_name = normalize_name(&name).map_err(|err| {
        (
            Some(Json(json!({
                "name": name,
                "error": err,
            }))),
            Status::BadRequest,
        )
    })?;

    let entries: Vec<models::Entry> = entries_db::table
        .filter(entries_db::name.eq(&name))
        .order(entries_db::created)
        .limit(1)
        .load::<models::Entry>(&*conn)
        .map_err(|_| (None, Status::InternalServerError))?;

    if entries.is_empty() {
        if let Some(in_progress_tasks) = worker.get_tasks_in_progress(&name) {
            JsonResult::Err(
                Some(Json(json!({
                    "name": normalized_name,
                    "in_progress": in_progress_tasks,
                }))),
                Status::TooManyRequests,
            )
        } else {
            match worker.launch_tasks(&name, None) {
                Ok((ref new_tasks, ref _in_progress_tasks)) if new_tasks.is_empty() => {
                    JsonResult::Err(
                        Some(Json(json!({
                            "name": normalized_name,
                            "error": "No recognized files",
                        }))),
                        Status::NotFound,
                    )
                }
                Ok((ref _new_tasks, ref in_progress_tasks)) => JsonResult::Err(
                    Some(Json(json!({
                            "name": normalized_name,
                            "in_progress": in_progress_tasks,
                        }))),
                    Status::Accepted,
                ),
                Err(error) => JsonResult::Err(
                    Some(Json(json!({
                            "name": normalized_name,
                            "error": error,
                        }))),
                    Status::BadRequest,
                ),
            }
        }
    } else {
        let entries = entries_db::table
            .filter(entries_db::name.eq(&name))
            .filter(sql("1 GROUP BY stat_kind, path")) // HACK: Diesel has no real group_by :(
            .order(entries_db::created)
            .load::<models::Entry>(&*conn)
            .map_err(|_| (None, Status::InternalServerError))?;
        JsonResult::Ok(Json(json!({
            "name": normalized_name,
            "stats": entries,
            "in_progress": worker.get_tasks_in_progress(&name).unwrap_or_else(|| vec![]),
        })))
    }
}

#[get("/<name>/<kind>")]
fn get_specific_stats(name: String, kind: String) -> JsonResult {
    let normalized_name = normalize_name(&name).map_err(|err| {
        (
            Some(Json(json!({
                "name": name,
                "error": err,
            }))),
            Status::BadRequest,
        )
    })?;

    let file_kind = FileKind::from_string(&kind).map_err(|err| {
        (
            Some(Json(json!({
                "name": name,
                "error": err,
            }))),
            Status::BadRequest,
        )
    });

    JsonResult::Ok(Json(json!({
        "name": normalized_name,
        "kind": format!("{:?}", file_kind),
    })))
    // TODO: implement this
}

#[post("/<name>")]
fn calculate_stats(name: String) -> String {
    name
    // TODO: implement this
}

#[post("/<name>/<kind>")]
fn calculate_specific_stats(name: String, kind: String) -> String {
    format!("{}: {}", name, kind)
    // TODO: implement this
}

fn main() {
    dotenv().ok();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = db::init_pool(&database_url);
    let worker = Worker::new(pool.clone());

    let cors_options = rocket_cors::Cors {
        allowed_origins: AllowedOrigins::all(),
        allowed_methods: vec![Method::Get, Method::Post]
            .into_iter()
            .map(From::from)
            .collect(),
        allowed_headers: AllowedHeaders::some(&["Authorization", "Accept"]),
        allow_credentials: true,
        ..Default::default()
    };

    rocket::ignite()
        .manage(pool)
        .manage(worker)
        .mount(
            "/",
            routes![
                index,
                get_stats,
                get_specific_stats,
                calculate_stats,
                calculate_specific_stats,
            ],
        )
        .attach(cors_options)
        .launch();
}
