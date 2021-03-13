#![feature(try_trait, proc_macro_hygiene, decl_macro)]
#![deny(clippy::all)]
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

#[macro_use]
extern crate diesel;

use std::{cmp::max, collections::HashSet, env, hash::BuildHasher, sync::Arc, thread, time::Duration};

use chrono::Utc;
use diesel::{prelude::*, sql_query, sql_types::Text};
use dotenv::dotenv;
use futures::{future::join_all, FutureExt};
use lazy_static::lazy_static;
use rocket::{
    get,
    http::{Accept, ContentType, MediaType, Method, Status},
    post,
    request::Form,
    response::Content,
    routes, State,
};
use rocket_contrib::{json, json::JsonValue};
use rocket_cors::{AllowedHeaders, AllowedOrigins};
use slog::{debug, error, o, Drain, Logger};
use tokio::runtime::{self, Runtime};

use db::DbConn;
use models::{FileKind, FileKindMapping, NewEntry};
use schema::entries as entries_db;
use util::{normalize_name, JsonResult, Params};
use worker::{Package, Task, Worker};

pub const ORGANIZATION_ROOT: &str = "https://github.com/apertium";
pub const ORGANIZATION_RAW_ROOT: &str = "https://raw.githubusercontent.com/apertium";
pub const GITHUB_GRAPHQL_API_ENDPOINT: &str = "https://api.github.com/graphql";
pub const PACKAGE_UPDATE_MIN_INTERVAL: Duration = Duration::from_secs(10);
pub const PACKAGE_UPDATE_FALLBACK_INTERVAL: Duration = Duration::from_secs(120);

lazy_static! {
    pub static ref RUNTIME: Runtime = runtime::Runtime::new().unwrap();
    pub static ref HTTPS_CLIENT: reqwest::Client = reqwest::Client::builder()
        .user_agent("apertium-stats-service")
        .build()
        .unwrap();
}

fn launch_tasks_and_reply(
    worker: &State<Arc<Worker>>,
    name: String,
    kind: Option<&FileKind>,
    options: Params,
) -> JsonResult {
    match RUNTIME.block_on(worker.build_tasks(&name, kind, options.is_recursive())) {
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
        Ok((_new_tasks, in_progress_tasks, futures)) => {
            if options.is_async() {
                let future_name = name.clone();
                let future_worker = (*worker).clone();
                RUNTIME.spawn(async move {
                    join_all(futures.into_iter().map(|future| {
                        future.map(|results| future_worker.handle_task_completion(&future_name, &results))
                    }))
                    .await
                });

                JsonResult::Err(
                    Some(json!({
                        "name": name,
                        "in_progress": in_progress_tasks,
                    })),
                    Status::Accepted,
                )
            } else {
                let futures = futures
                    .into_iter()
                    .map(|future| future.map(|results| worker.handle_task_completion(&name, &results)));
                let result = RUNTIME.block_on(join_all(futures));
                let stats: Vec<&NewEntry> = result.iter().flatten().collect();
                JsonResult::Ok(json!({
                    "name": name,
                    "stats": stats,
                    "in_progress": vec![] as Vec<Task>,
                }))
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

async fn update_packages(worker: State<'_, Arc<Worker>>, query: Option<String>) -> JsonResult {
    if let Err(err) = worker.update_packages().await {
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
fn index(accept: Option<&Accept>) -> Content<&str> {
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
        // Diesel doesn't support self JOINs or GROUP BY :(
        let entries: Vec<models::Entry> = sql_query(
            "
                SELECT *
                FROM entries e1
                JOIN (
                    SELECT id, MAX(created)
                    FROM entries
                    WHERE name = ?
                    GROUP BY stat_kind, path
                ) e2
                ON e1.id = e2.id
            ",
        )
        .bind::<Text, _>(&name)
        .load(&*conn)
        .map_err(|err| handle_db_error(&worker.logger, err))?;

        JsonResult::Ok(json!({
            "name": name,
            "stats": entries,
            "in_progress": worker.get_tasks_in_progress(&name).unwrap_or_else(Vec::new),
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
        // Diesel doesn't support self JOINs or GROUP BY :(
        let entries: Vec<models::Entry> = sql_query(
            "
                SELECT *
                FROM entries e1
                JOIN (
                    SELECT id, MAX(created)
                    FROM entries
                    WHERE name = ? AND file_kind = ?
                    GROUP BY stat_kind, path
                ) e2
                ON e1.id = e2.id
            ",
        )
        .bind::<Text, _>(&name)
        .bind::<FileKindMapping, _>(&file_kind)
        .load(&*conn)
        .map_err(|err| handle_db_error(&worker.logger, err))?;

        JsonResult::Ok(json!({
            "name": name,
            "stats": entries,
            "in_progress": worker.get_tasks_in_progress(&name).unwrap_or_else(Vec::new),
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
    RUNTIME.block_on(update_packages(worker, None))
}

#[post("/packages/<query>")]
fn update_specific_packages(worker: State<Arc<Worker>>, query: String) -> JsonResult {
    RUNTIME.block_on(update_packages(worker, Some(query)))
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

fn start_package_update_loop(worker: Arc<Worker>) {
    let initial_delay = {
        match RUNTIME.block_on(worker.update_packages()) {
            Ok(interval) => max(interval, PACKAGE_UPDATE_MIN_INTERVAL),
            Err(err) => panic!("Failed to initialize package list: {:?}", err),
        }
    };
    worker.record_next_packages_update(initial_delay);

    thread::spawn(move || loop {
        thread::sleep(initial_delay);

        let next_update = {
            match RUNTIME.block_on(worker.update_packages()) {
                Ok(interval) => max(interval, PACKAGE_UPDATE_MIN_INTERVAL),
                Err(err) => {
                    error!(worker.logger, "Failed to update packages: {:?}", err);
                    PACKAGE_UPDATE_FALLBACK_INTERVAL
                },
            }
        };

        let mut update_scheduled = Utc::now().naive_utc();
        worker.record_next_packages_update(next_update);
        loop {
            thread::sleep(next_update);

            if worker.packages_updated.read().unwrap().unwrap() > update_scheduled {
                debug!(worker.logger, "Delaying scheduled package update {:?}", next_update);
                update_scheduled = Utc::now().naive_utc();
                worker.record_next_packages_update(next_update);
            } else {
                break;
            }
        }
    });
}

pub fn service(
    database_url: String,
    github_auth_token: Option<&str>,
    github_graphql_api_endpoint: Option<&str>,
) -> rocket::Rocket {
    let pool = db::init_pool(&database_url);
    let logger = create_logger();
    let worker = Arc::new(Worker::new(
        pool.clone(),
        logger.clone(),
        github_auth_token.map(str::to_owned),
        github_graphql_api_endpoint
            .unwrap_or(GITHUB_GRAPHQL_API_ENDPOINT)
            .to_owned(),
    ));

    let package_listing_routes_enabled = match github_auth_token {
        Some(_) => {
            start_package_update_loop(worker.clone());
            true
        },
        None => false,
    };

    rocket(pool, worker, logger, package_listing_routes_enabled)
}

#[cfg(not(tarpaulin_include))]
fn main() {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let github_auth_token = env::var("GITHUB_AUTH_TOKEN").map(Some).unwrap_or_default();
    if github_auth_token.is_none() {
        eprintln!("GITHUB_AUTH_TOKEN environment variable not set -- /packages route will be unavailable");
    }

    service(database_url, github_auth_token.as_deref(), None).launch();
}
