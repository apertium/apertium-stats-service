use std::{
    collections::{hash_map::Entry, HashMap}, process::{Command, Output}, str, sync::{Arc, Mutex},
};

use chrono::{NaiveDateTime, Utc};
use diesel::{self, RunQueryDsl};
use hyper::{client::connect::HttpConnector, Client};
use hyper_tls::HttpsConnector;
use quick_xml::{
    events::{attributes::Attribute, Event}, Reader,
};
use slog::Logger;
use tokio::prelude::{future::join_all, Future};

use db::Pool;
use models::{FileKind, NewEntry};
use schema::entries;
use stats::{get_file_kind, get_file_stats};
use ORGANIZATION_ROOT;

#[derive(Serialize, Clone, Debug)]
pub struct Task {
    pub kind: FileKind,
    pub path: String,
    pub revision: i32,
    pub created: NaiveDateTime,
}
type Tasks = Vec<Task>;

pub struct Worker {
    pool: Pool,
    current_tasks: Arc<Mutex<HashMap<String, Tasks>>>,
    logger: Logger,
}

fn list_files(name: &str, recursive: bool) -> Result<Vec<(String, i32)>, String> {
    fn handle_utf8_error(err: str::Utf8Error, reader: &Reader<&[u8]>) -> String {
        format!(
            "UTF8 decoding error at position {}: {:?}",
            reader.buffer_position(),
            err,
        )
    }

    let output = Command::new("svn")
        .arg("list")
        .arg("--xml")
        .args(if recursive {
            vec!["--recursive"]
        } else {
            vec![]
        })
        .arg(format!("{}/{}/trunk", ORGANIZATION_ROOT, name))
        .output();

    match output {
        Ok(Output {
            status, ref stdout, ..
        }) if status.success() =>
        {
            let xml = String::from_utf8_lossy(stdout);
            let mut reader = Reader::from_str(&xml);
            let mut buf = Vec::new();

            let mut files = Vec::new();
            let mut in_name = false;
            let mut name = String::new();

            loop {
                match reader.read_event(&mut buf) {
                    Ok(Event::Start(ref e)) if e.name() == b"name" => in_name = true,
                    Ok(Event::Start(ref e)) if e.name() == b"commit" => {
                        for attr in e.attributes() {
                            if let Ok(Attribute { value, key }) = attr {
                                if str::from_utf8(key)
                                    .map_err(|err| handle_utf8_error(err, &reader))?
                                    == "revision"
                                {
                                    files.push((
                                        name.clone(),
                                        str::from_utf8(&value)
                                            .map_err(|err| handle_utf8_error(err, &reader))?
                                            .parse::<i32>()
                                            .map_err(|err| {
                                                format!(
                                                    "Revision number parsing error at position {}: {:?}",
                                                    reader.buffer_position(),
                                                    err,
                                                )
                                            })?,
                                    ));
                                    break;
                                }
                            }
                        }
                    },
                    Ok(Event::Text(ref e)) if in_name => {
                        name = e.unescape_and_decode(&reader).map_err(|err| {
                            format!(
                                "Decoding error at position {}: {:?}",
                                reader.buffer_position(),
                                err,
                            )
                        })?
                    },
                    Ok(Event::End(ref e)) if e.name() == b"name" => in_name = false,
                    Ok(Event::Eof) => break,
                    Err(err) => panic!("Error at position {}: {:?}", reader.buffer_position(), err),
                    _ => (),
                }
                buf.clear();
            }

            Ok(files)
        }
        Ok(Output { stderr, .. }) => {
            let error = String::from_utf8_lossy(&stderr);
            Err(format!("Package not found: {}", error))
        },
        Err(_) => Err(format!("Package search failed: {}", name)),
    }
}

impl Worker {
    pub fn new(pool: Pool, logger: Logger) -> Worker {
        Worker {
            pool,
            current_tasks: Arc::new(Mutex::new(HashMap::new())),
            logger,
        }
    }

    pub fn get_tasks_in_progress(&self, name: &str) -> Option<Tasks> {
        let current_tasks = self.current_tasks.lock().unwrap();
        current_tasks.get(name).cloned()
    }

    pub fn launch_tasks(
        &self,
        client: &Client<HttpsConnector<HttpConnector>>,
        name: &str,
        maybe_kind: Option<&FileKind>,
        recursive: bool,
    ) -> Result<(Tasks, Tasks, impl Future<Item = Vec<NewEntry>>), String> {
        let logger = self.logger.new(o!(
            "package" => name.to_string().clone(),
            "recursive" => recursive,
        ));

        list_files(name, recursive).and_then(|files| {
            let mut current_tasks = self.current_tasks.lock().unwrap();
            let current_package_tasks = current_tasks.entry(name.to_string());

            let new_tasks = files
                .into_iter()
                .filter_map(|(file, revision)| {
                    get_file_kind(&file).and_then(|file_kind| {
                        let requested_kind = maybe_kind.map_or(true, |kind| kind == &file_kind);
                        let in_progress = match current_package_tasks {
                            Entry::Occupied(ref occupied) => occupied
                                .get()
                                .into_iter()
                                .any(|Task { kind, path, .. }| kind == &file_kind && path == &file),
                            _ => false,
                        };
                        if requested_kind && !in_progress {
                            Some(Task {
                                kind: file_kind,
                                path: file.to_string(),
                                created: Utc::now().naive_utc(),
                                revision,
                            })
                        } else {
                            None
                        }
                    })
                })
                .collect::<Vec<_>>();
            info!(
                logger,
                "Spawning {} tasks: {:?}",
                new_tasks.len(),
                new_tasks,
            );

            let future = join_all(
                new_tasks
                    .iter()
                    .map(|task| self.launch_task(&logger, client, name, task))
                    .collect::<Vec<_>>(),
            ).map(|entries| entries.into_iter().flat_map(|x| x).collect());
            let (new_tasks, in_progress_tasks) =
                Worker::record_new_tasks(current_package_tasks, new_tasks)?;

            Ok((new_tasks, in_progress_tasks, future))
        })
    }

    fn launch_task(
        &self,
        logger: &Logger,
        client: &Client<HttpsConnector<HttpConnector>>,
        package_name: &str,
        task: &Task,
    ) -> impl Future<Item = Vec<NewEntry>, Error = ()> {
        let current_tasks_guard = self.current_tasks.clone();
        let pool = self.pool.clone();
        let task = task.clone();
        let package_name = package_name.to_string();
        let logger = logger.new(o!(
            "path" => task.path.clone(),
            "kind" => task.kind.to_string(),
        ));

        get_file_stats(
            &logger,
            &client,
            task.path.clone(),
            &package_name,
            task.kind.clone(),
        ).then(move |maybe_stats| {
            let mut current_tasks = current_tasks_guard.lock().unwrap();
            Worker::record_task_completion(current_tasks.entry(package_name.clone()), &task);

            match maybe_stats {
                Ok(stats) => {
                    debug!(logger, "Completed executing task");
                    let conn = pool.get().expect("database connection");
                    let new_entries = stats
                        .into_iter()
                        .map(|(kind, value)| NewEntry {
                            name: package_name.clone(),
                            created: Utc::now().naive_utc(),
                            requested: task.created,
                            path: task.path.clone(),
                            stat_kind: kind,
                            file_kind: task.kind.clone(),
                            value: value.into(),
                            revision: task.revision,
                        })
                        .collect::<Vec<_>>();
                    diesel::insert_into(entries::table)
                        .values(&new_entries)
                        .execute(&*conn)
                        .unwrap();

                    Ok(new_entries)
                },
                Err(err) => {
                    error!(logger, "Error executing task: {:?}", err);
                    Ok(vec![])
                },
            }
        })
    }

    fn record_task_completion(current_package_tasks: Entry<String, Tasks>, task: &Task) {
        if let Entry::Occupied(mut occupied) = current_package_tasks {
            if let Some(position) = occupied.get().iter().position(
                |&Task {
                     ref kind, ref path, ..
                 }| { kind == &task.kind && path == &task.path },
            ) {
                occupied.get_mut().remove(position);
                if occupied.get().is_empty() {
                    occupied.remove_entry();
                }
            }
        }
    }

    fn record_new_tasks(
        current_package_tasks: Entry<String, Tasks>,
        new_tasks: Tasks,
    ) -> Result<(Tasks, Tasks), String> {
        match current_package_tasks {
            Entry::Occupied(mut occupied) => {
                if !new_tasks.is_empty() {
                    occupied.get_mut().extend(new_tasks.clone());
                }
                Ok((new_tasks, occupied.get().to_vec()))
            },
            Entry::Vacant(vacant) => {
                if new_tasks.is_empty() {
                    Ok((new_tasks, Vec::new()))
                } else {
                    Ok((new_tasks.clone(), vacant.insert(new_tasks).clone()))
                }
            },
        }
    }
}
