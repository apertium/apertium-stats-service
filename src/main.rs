#![feature(plugin, custom_attribute, try_trait, custom_derive)]
#![plugin(rocket_codegen)]
#![deny(clippy)]
#![allow(needless_pass_by_value)]
#![allow(suspicious_else_formatting)]
#![allow(print_literal)]

mod db;
mod models;
mod schema;
mod stats;
mod util;
mod worker;

#[cfg(test)]
#[macro_use]
mod tests;

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
extern crate tokio;
extern crate tokio_core;

use std::env;

use diesel::dsl::sql;
use diesel::prelude::*;
use dotenv::dotenv;
use rocket::http::{Method, Status};
use rocket::State;
use rocket_contrib::{Json, Value};
use rocket_cors::{AllowedHeaders, AllowedOrigins};
use tokio::prelude::Future;
use tokio::runtime::Runtime;
use tokio_core::reactor::Core;

use db::DbConn;
use models::FileKind;
use schema::entries as entries_db;
use util::{normalize_name, JsonResult, Params};
use worker::{Task, Worker};

pub const ORGANIZATION_ROOT: &str = "https://github.com/apertium";
pub const ORGANIZATION_RAW_ROOT: &str = "https://raw.githubusercontent.com/apertium";
pub const LANG_CODE_RE: &str = r"\w{2,3}(_\w+)?";

lazy_static! {
    static ref RUNTIME: Runtime = Runtime::new().unwrap();
}

fn launch_tasks_and_reply(
    worker: &State<Worker>,
    name: String,
    kind: Option<&FileKind>,
    options: Params,
) -> JsonResult {
    match worker.launch_tasks(&RUNTIME, &name, kind, options.is_recursive()) {
        Ok((ref new_tasks, ref in_progress_tasks, ref _future))
            if new_tasks.is_empty() && in_progress_tasks.is_empty() =>
        {
            JsonResult::Err(
                Some(Json(json!({
                    "name": name,
                    "error": "No recognized files",
                }))),
                Status::NotFound,
            )
        }
        Ok((_new_tasks, in_progress_tasks, future)) => {
            if options.is_async() {
                let detached_future = future.map(|_| ()).map_err(|_| ());
                RUNTIME.executor().spawn(detached_future);

                JsonResult::Err(
                    Some(Json(json!({
                            "name": name,
                            "in_progress": in_progress_tasks,
                        }))),
                    Status::Accepted,
                )
            } else {
                let mut core = Core::new().unwrap(); // TODO: stop using tokio-core

                match core.run(future) {
                    Ok(stats) => JsonResult::Ok(Json(json!({
                        "name": name,
                        "stats": stats,
                        "in_progress": vec![] as Vec<Task>,
                    }))),
                    Err(_err) => JsonResult::Err(None, Status::InternalServerError),
                }
            }
        }
        Err(error) => JsonResult::Err(
            Some(Json(json!({
                    "name": name,
                    "error": error,
                }))),
            Status::BadRequest,
        ),
    }
}

fn parse_name_param(name: &str) -> Result<String, (Option<Json<Value>>, Status)> {
    normalize_name(name).map_err(|err| {
        (
            Some(Json(json!({
                "name": name,
                "error": err,
            }))),
            Status::BadRequest,
        )
    })
}

fn parse_kind_param(name: &str, kind: &str) -> Result<FileKind, (Option<Json<Value>>, Status)> {
    FileKind::from_string(kind).map_err(|err| {
        (
            Some(Json(json!({
                "name": name,
                "error": err,
            }))),
            Status::BadRequest,
        )
    })
}

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
calculates <kind> statistics for the specified package

See openapi.yaml for full specification."
}

#[get("/<name>?<params>")]
fn get_stats(
    name: String,
    params: Option<Params>,
    conn: DbConn,
    worker: State<Worker>,
) -> JsonResult {
    let name = parse_name_param(&name)?;

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
                    "name": name,
                    "in_progress": in_progress_tasks,
                }))),
                Status::TooManyRequests,
            )
        } else {
            drop(conn);
            launch_tasks_and_reply(&worker, name, None, params.unwrap_or_default())
        }
    } else {
        let entries = entries_db::table
            .filter(entries_db::name.eq(&name))
            .filter(sql("1 GROUP BY stat_kind, path")) // HACK: Diesel has no real group_by :(
            .order(entries_db::created)
            .load::<models::Entry>(&*conn)
            .map_err(|_| (None, Status::InternalServerError))?;
        JsonResult::Ok(Json(json!({
            "name": name,
            "stats": entries,
            "in_progress": worker.get_tasks_in_progress(&name).unwrap_or_else(|| vec![]),
        })))
    }
}

// the no_params equivalents are required due to https://github.com/SergioBenitez/Rocket/issues/376
#[get("/<name>")]
fn get_stats_no_params(name: String, conn: DbConn, worker: State<Worker>) -> JsonResult {
    get_stats(name, None, conn, worker)
}

#[get("/<name>/<kind>?<params>")]
fn get_specific_stats(
    name: String,
    kind: String,
    params: Option<Params>,
    conn: DbConn,
    worker: State<Worker>,
) -> JsonResult {
    let name = parse_name_param(&name)?;
    let file_kind = parse_kind_param(&name, &kind)?;

    let entries: Vec<models::Entry> = entries_db::table
        .filter(entries_db::name.eq(&name))
        .filter(entries_db::file_kind.eq(&file_kind))
        .order(entries_db::created)
        .limit(1)
        .load::<models::Entry>(&*conn)
        .map_err(|_| (None, Status::InternalServerError))?;

    if entries.is_empty() {
        if let Some(in_progress_tasks) = worker.get_tasks_in_progress(&name) {
            if in_progress_tasks
                .iter()
                .filter(|task| task.kind == file_kind)
                .count() != 0
            {
                return JsonResult::Err(
                    Some(Json(json!({
                        "name": name,
                        "in_progress": in_progress_tasks,
                    }))),
                    Status::TooManyRequests,
                );
            }
        }

        drop(conn);
        launch_tasks_and_reply(&worker, name, Some(&file_kind), params.unwrap_or_default())
    } else {
        let entries = entries_db::table
            .filter(entries_db::name.eq(&name))
            .filter(entries_db::file_kind.eq(&file_kind))
            .filter(sql("1 GROUP BY stat_kind, path")) // HACK: Diesel has no real group_by :(
            .order(entries_db::created)
            .load::<models::Entry>(&*conn)
            .map_err(|_| (None, Status::InternalServerError))?;
        JsonResult::Ok(Json(json!({
            "name": name,
            "stats": entries,
            "in_progress": worker.get_tasks_in_progress(&name).unwrap_or_else(|| vec![]),
        })))
    }
}

#[get("/<name>/<kind>")]
fn get_specific_stats_no_params(
    name: String,
    kind: String,
    conn: DbConn,
    worker: State<Worker>,
) -> JsonResult {
    get_specific_stats(name, kind, None, conn, worker)
}

#[post("/<name>?<params>")]
fn calculate_stats(name: String, params: Option<Params>, worker: State<Worker>) -> JsonResult {
    let name = parse_name_param(&name)?;
    launch_tasks_and_reply(&worker, name, None, params.unwrap_or_default())
}

#[post("/<name>")]
fn calculate_stats_no_params(name: String, worker: State<Worker>) -> JsonResult {
    calculate_stats(name, None, worker)
}

#[post("/<name>/<kind>?<params>")]
fn calculate_specific_stats(
    name: String,
    kind: String,
    params: Option<Params>,
    worker: State<Worker>,
) -> JsonResult {
    let name = parse_name_param(&name)?;
    let file_kind = parse_kind_param(&name, &kind)?;
    launch_tasks_and_reply(&worker, name, Some(&file_kind), params.unwrap_or_default())
}

#[post("/<name>/<kind>")]
fn calculate_specific_stats_no_params(
    name: String,
    kind: String,
    worker: State<Worker>,
) -> JsonResult {
    calculate_specific_stats(name, kind, None, worker)
}

fn rocket(database_url: String) -> rocket::Rocket {
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
                get_stats_no_params,
                get_specific_stats,
                get_specific_stats_no_params,
                calculate_stats,
                calculate_stats_no_params,
                calculate_specific_stats,
                calculate_specific_stats_no_params,
            ],
        )
        .attach(cors_options)
}

fn main() {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    rocket(database_url).launch();
}
