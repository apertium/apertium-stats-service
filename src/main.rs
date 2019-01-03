#![feature(custom_attribute, try_trait, custom_derive, proc_macro_hygiene, decl_macro)]
#![deny(clippy::all)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::suspicious_else_formatting)]
#![allow(proc_macro_derive_resolution_fallback)]

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
extern crate failure;
extern crate slog_envlogger;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_derive_enum;
extern crate dotenv;
#[macro_use]
extern crate lazy_static;
extern crate regex;
#[macro_use]
extern crate rocket;
extern crate serde;
extern crate tempfile;
#[macro_use]
extern crate rocket_contrib;
extern crate rocket_cors;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate tokio;
extern crate tokio_process;
#[macro_use]
extern crate slog;
extern crate graphql_client;
extern crate hyper;
extern crate hyper_tls;
extern crate quick_xml;
extern crate slog_async;
extern crate slog_term;

use std::{cmp::max, collections::HashSet, env, hash::BuildHasher, sync::Arc, thread, time::Duration};

use diesel::{dsl::sql, prelude::*};
use dotenv::dotenv;
use hyper::{client::connect::HttpConnector, Client};
use hyper_tls::HttpsConnector;
use rocket::{
    http::{Accept, ContentType, MediaType, Method, Status},
    request::Form,
    response::Content,
    State,
};
use rocket_contrib::json::JsonValue;
use rocket_cors::{AllowedHeaders, AllowedOrigins};
use slog::{Drain, Logger};
use tokio::{executor::current_thread::CurrentThread, prelude::Future, runtime::Runtime};

use db::DbConn;
use models::FileKind;
use schema::entries as entries_db;
use util::{normalize_name, JsonResult, Params};
use worker::{Package, Task, Worker};

pub const ORGANIZATION_ROOT: &str = "https://github.com/apertium";
pub const ORGANIZATION_RAW_ROOT: &str = "https://raw.githubusercontent.com/apertium";
pub const GITHUB_API_REPOS_ENDPOINT: &str = "https://api.github.com/orgs/apertium/repos";
pub const GITHUB_GRAPHQL_API_ENDPOINT: &str = "https://api.github.com/graphql";
pub const PACKAGE_UPDATE_MIN_INTERVAL: Duration = Duration::from_secs(10);
pub const PACKAGE_UPDATE_FALLBACK_INTERVAL: Duration = Duration::from_secs(120);

lazy_static! {
    pub static ref RUNTIME: Runtime = Runtime::new().unwrap();
    pub static ref HTTPS_CLIENT: reqwest::Client = reqwest::Client::new();
    pub static ref HYPER_HTTPS_CLIENT: Client<HttpsConnector<HttpConnector>> = Client::builder()
        .executor(RUNTIME.executor())
        .build(HttpsConnector::new(4).unwrap());
}

fn launch_tasks_and_reply(
    worker: &State<Arc<Worker>>,
    name: String,
    kind: Option<&FileKind>,
    options: Params,
) -> JsonResult {
    match worker.launch_tasks(&name, kind, options.is_recursive()) {
        Ok((ref new_tasks, ref in_progress_tasks, ref _future))
            if new_tasks.is_empty() && in_progress_tasks.is_empty() =>
        {
            JsonResult::Err(
                Some(json!({
                    "name": name,
                    "error": "No recognized files",
                })),
                Status::NotFound,
            )
        },
        Ok((_new_tasks, in_progress_tasks, future)) => {
            if options.is_async() {
                let detached_future = future.map(|_| ()).map_err(|_| ());
                RUNTIME.executor().spawn(detached_future);

                JsonResult::Err(
                    Some(json!({
                        "name": name,
                        "in_progress": in_progress_tasks,
                    })),
                    Status::Accepted,
                )
            } else {
                match CurrentThread::new().block_on(future) {
                    Ok(stats) => JsonResult::Ok(json!({
                        "name": name,
                        "stats": stats,
                        "in_progress": vec![] as Vec<Task>,
                    })),
                    Err(_err) => {
                        error!(worker.logger, "Failed to run tasks to completion"; "name" => name);
                        JsonResult::Err(None, Status::InternalServerError)
                    },
                }
            }
        },
        Err(error) => JsonResult::Err(
            Some(json!({
                "name": name,
                "error": error,
            })),
            Status::BadRequest,
        ),
    }
}

fn parse_name_param<H: BuildHasher>(
    name: &str,
    package_names: HashSet<String, H>,
) -> Result<String, (Option<JsonValue>, Status)> {
    normalize_name(name, package_names).map_err(|err| {
        (
            Some(json!({
                "name": name,
                "error": err,
            })),
            Status::BadRequest,
        )
    })
}

fn parse_kind_param(name: &str, kind: &str) -> Result<FileKind, (Option<JsonValue>, Status)> {
    FileKind::from_string(kind).map_err(|err| {
        (
            Some(json!({
                "name": name,
                "error": err,
            })),
            Status::BadRequest,
        )
    })
}

fn handle_db_error(logger: &Logger, err: diesel::result::Error) -> (Option<JsonValue>, Status) {
    error!(logger, "Encountered database level error: {:?}", err);
    (None, Status::InternalServerError)
}

#[allow(clippy::clone_on_copy)]
fn get_packages(worker: State<Arc<Worker>>, query: Option<String>) -> JsonResult {
    let lower_query = query.map(|x| x.to_ascii_lowercase());
    let packages = worker.packages.read().unwrap().clone();
    JsonResult::Ok(json!({
        "packages": match lower_query {
            Some(q) => packages.into_iter().filter(|Package {name, ..}| name.to_ascii_lowercase().contains(&q)).collect(),
            None => packages
        },
        "as_of": worker.packages_updated.read().unwrap().clone(),
        "next_update": worker.packages_next_update.read().unwrap().clone(),
    }))
}

fn update_packages(worker: State<Arc<Worker>>, query: Option<String>) -> JsonResult {
    if let Err(err) = worker.update_packages() {
        error!(worker.logger, "Failed to update packages: {:?}", err);
        return JsonResult::Err(
            Some(json!({
                "error": err.to_string(),
            })),
            Status::InternalServerError,
        );
    }

    get_packages(worker, query)
}

fn get_package_names(worker: &State<Arc<Worker>>) -> HashSet<String> {
    worker
        .packages
        .read()
        .unwrap()
        .iter()
        .map(|Package { name, .. }| name.to_string())
        .collect()
}

#[get("/")]
fn index<'a>(accept: Option<&'a Accept>) -> Content<&'a str> {
    if accept.map_or(false, |a| a.preferred().media_type() == &MediaType::HTML) {
        Content(ContentType::HTML, include_str!("../index.html"))
    } else {
        Content(
            ContentType::Plain,
            "USAGE

GET /apertium-<code1>(-<code2>)
retrieves statistics for the specified package

GET /apertium-<code1>(-<code2>)/<kind>
retrieves <kind> statistics for the specified package

POST /apertium-<code1>(-<code2>)
calculates statistics for the specified package

POST /apertium-<code1>(-<code2>)/<kind>
calculates <kind> statistics for the specified package

GET /packages/<?query>
lists packages with names including the optional query

POST /packages/<?query>
updates package cache and lists specified packages

See /openapi.yaml for full specification.",
        )
    }
}

#[get("/openapi.yaml")]
fn openapi_yaml() -> Content<&'static str> {
    Content(
        ContentType::new("application", "x-yaml"),
        include_str!("../openapi.yaml"),
    )
}

#[get("/<name>?<params..>", rank = 1)]
fn get_stats(name: String, params: Form<Option<Params>>, conn: DbConn, worker: State<Arc<Worker>>) -> JsonResult {
    let name = parse_name_param(&name, get_package_names(&worker))?;

    let entries: Vec<models::Entry> = entries_db::table
        .filter(entries_db::name.eq(&name))
        .order(entries_db::created)
        .limit(1)
        .load::<models::Entry>(&*conn)
        .map_err(|err| handle_db_error(&worker.logger, err))?;

    if entries.is_empty() {
        if let Some(in_progress_tasks) = worker.get_tasks_in_progress(&name) {
            JsonResult::Err(
                Some(json!({
                    "name": name,
                    "in_progress": in_progress_tasks,
                })),
                Status::TooManyRequests,
            )
        } else {
            drop(conn);
            launch_tasks_and_reply(&worker, name, None, params.into_inner().unwrap_or_default())
        }
    } else {
        let entries = entries_db::table
            .filter(entries_db::name.eq(&name))
            .filter(sql("1 GROUP BY stat_kind, path")) // HACK: Diesel has no real group_by :(
            .order(entries_db::created)
            .load::<models::Entry>(&*conn)
            .map_err(|err| handle_db_error(&worker.logger, err))?;
        JsonResult::Ok(json!({
            "name": name,
            "stats": entries,
            "in_progress": worker.get_tasks_in_progress(&name).unwrap_or_else(|| vec![]),
        }))
    }
}

#[get("/<name>/<kind>?<params..>", rank = 1)]
fn get_specific_stats(
    name: String,
    kind: String,
    params: Form<Option<Params>>,
    conn: DbConn,
    worker: State<Arc<Worker>>,
) -> JsonResult {
    let name = parse_name_param(&name, get_package_names(&worker))?;
    let file_kind = parse_kind_param(&name, &kind)?;

    let entries: Vec<models::Entry> = entries_db::table
        .filter(entries_db::name.eq(&name))
        .filter(entries_db::file_kind.eq(&file_kind))
        .order(entries_db::created)
        .limit(1)
        .load::<models::Entry>(&*conn)
        .map_err(|err| handle_db_error(&worker.logger, err))?;

    if entries.is_empty() {
        if let Some(in_progress_tasks) = worker.get_tasks_in_progress(&name) {
            if in_progress_tasks.iter().filter(|task| task.kind == file_kind).count() != 0 {
                return JsonResult::Err(
                    Some(json!({
                        "name": name,
                        "in_progress": in_progress_tasks,
                    })),
                    Status::TooManyRequests,
                );
            }
        }

        drop(conn);
        launch_tasks_and_reply(&worker, name, Some(&file_kind), params.into_inner().unwrap_or_default())
    } else {
        let entries = entries_db::table
            .filter(entries_db::name.eq(&name))
            .filter(entries_db::file_kind.eq(&file_kind))
            .filter(sql("1 GROUP BY stat_kind, path")) // HACK: Diesel has no real group_by :(
            .order(entries_db::created)
            .load::<models::Entry>(&*conn)
            .map_err(|err| handle_db_error(&worker.logger, err))?;
        JsonResult::Ok(json!({
            "name": name,
            "stats": entries,
            "in_progress": worker.get_tasks_in_progress(&name).unwrap_or_else(|| vec![]),
        }))
    }
}

#[post("/<name>?<params..>", rank = 1)]
fn calculate_stats(name: String, params: Form<Option<Params>>, worker: State<Arc<Worker>>) -> JsonResult {
    let name = parse_name_param(&name, get_package_names(&worker))?;
    launch_tasks_and_reply(&worker, name, None, params.into_inner().unwrap_or_default())
}

#[post("/<name>/<kind>?<params..>", rank = 1)]
fn calculate_specific_stats(
    name: String,
    kind: String,
    params: Form<Option<Params>>,
    worker: State<Arc<Worker>>,
) -> JsonResult {
    let name = parse_name_param(&name, get_package_names(&worker))?;
    let file_kind = parse_kind_param(&name, &kind)?;
    launch_tasks_and_reply(&worker, name, Some(&file_kind), params.into_inner().unwrap_or_default())
}

#[get("/packages")]
fn get_all_packages(worker: State<Arc<Worker>>) -> JsonResult {
    get_packages(worker, None)
}

#[get("/packages/<query>")]
fn get_specific_packages(worker: State<Arc<Worker>>, query: String) -> JsonResult {
    get_packages(worker, Some(query))
}

#[post("/packages")]
fn update_all_packages(worker: State<Arc<Worker>>) -> JsonResult {
    update_packages(worker, None)
}

#[post("/packages/<query>")]
fn update_specific_packages(worker: State<Arc<Worker>>, query: String) -> JsonResult {
    update_packages(worker, Some(query))
}

fn create_logger() -> Logger {
    let decorator = slog_term::TermDecorator::new().stderr().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let env_drain = slog_envlogger::new(drain);
    let async_drain = slog_async::Async::new(env_drain).build().fuse();
    Logger::root(async_drain, o!())
}

fn rocket(pool: db::Pool, worker: Arc<Worker>, logger: Logger, package_listing_routes_enabled: bool) -> rocket::Rocket {
    let cors_options = rocket_cors::Cors {
        allowed_origins: AllowedOrigins::all(),
        allowed_methods: vec![Method::Get, Method::Post].into_iter().map(From::from).collect(),
        allowed_headers: AllowedHeaders::some(&["Authorization", "Accept"]),
        allow_credentials: true,
        ..Default::default()
    };

    let mut routes = routes![
        index,
        openapi_yaml,
        get_stats,
        get_specific_stats,
        calculate_stats,
        calculate_specific_stats,
        get_all_packages,
        get_specific_packages,
        update_all_packages,
        update_specific_packages,
    ];
    if !package_listing_routes_enabled {
        routes = routes
            .into_iter()
            .filter(|route| !route.uri.path().starts_with("/packages"))
            .collect();
    }

    rocket::ignite()
        .manage(pool)
        .manage(worker)
        .manage(logger)
        .mount("/", routes)
        .attach(cors_options)
}

pub fn service(database_url: String, github_auth_token: Option<String>) -> rocket::Rocket {
    let pool = db::init_pool(&database_url);
    let logger = create_logger();
    let package_listing_routes_enabled = github_auth_token.is_some();
    let worker = Arc::new(Worker::new(pool.clone(), logger.clone(), github_auth_token));

    if package_listing_routes_enabled {
        let initial_delay = {
            match worker.update_packages() {
                Ok(interval) => max(interval, PACKAGE_UPDATE_MIN_INTERVAL),
                Err(err) => panic!("Failed to initialize package list: {:?}", err),
            }
        };
        worker.record_next_packages_update(initial_delay);

        let package_update_worker = worker.clone();
        thread::spawn(move || loop {
            thread::sleep(initial_delay);
            let next_update = {
                match package_update_worker.update_packages() {
                    Ok(interval) => max(interval, PACKAGE_UPDATE_MIN_INTERVAL),
                    Err(err) => {
                        error!(package_update_worker.logger, "Failed to update packages: {:?}", err);
                        PACKAGE_UPDATE_FALLBACK_INTERVAL
                    },
                }
            };
            package_update_worker.record_next_packages_update(next_update);
            thread::sleep(next_update);
        });
    }

    rocket(pool, worker, logger, package_listing_routes_enabled)
}

#[cfg_attr(tarpaulin, skip)]
fn main() {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let github_auth_token = env::var("GITHUB_AUTH_TOKEN").map(Some).unwrap_or_default();
    if github_auth_token.is_none() {
        eprintln!("GITHUB_AUTH_TOKEN environment variable not set -- /packages route will be unavailable");
    }

    service(database_url, github_auth_token).launch();;
}
