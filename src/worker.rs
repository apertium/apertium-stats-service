extern crate diesel;
extern crate hyper;
extern crate hyper_tls;
extern crate quick_xml;

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::process::{Command, Output};
use std::str;
use std::sync::{Arc, Mutex};

use self::diesel::prelude::*;
use futures::future::join_all;
use futures::Future;
use self::hyper::Client;
use self::hyper_tls::HttpsConnector;
use self::quick_xml::events::attributes::Attribute;
use self::quick_xml::events::Event;
use self::quick_xml::reader::Reader;
use chrono::{NaiveDateTime, Utc};
use tokio_core::reactor::Core;

use super::models::{FileKind, NewEntry};
use super::schema::entries;
use super::stats::{get_file_kind, get_file_stats};

#[derive(Serialize, Clone)]
pub struct Task {
    pub kind: FileKind,
    pub path: String,
    pub revision: i32,
    pub created: NaiveDateTime,
}
type Tasks = Vec<Task>;

pub struct Worker {
    pool: super::db::Pool,
    current_tasks: Arc<Mutex<HashMap<String, Tasks>>>,
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
        .arg(format!("{}/{}/trunk", super::ORGANIZATION_ROOT, name))
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
                    }
                    Ok(Event::Text(ref e)) if in_name => {
                        name = e.unescape_and_decode(&reader).map_err(|err| {
                            format!(
                                "Decoding error at position {}: {:?}",
                                reader.buffer_position(),
                                err,
                            )
                        })?
                    }
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
        }
        Err(_) => Err(format!("Package search failed: {}", name)),
    }
}

impl Worker {
    pub fn new(pool: super::db::Pool) -> Worker {
        Worker {
            pool,
            current_tasks: Arc::new(Mutex::new(HashMap::new())),
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
            }
            Entry::Vacant(vacant) => {
                if new_tasks.is_empty() {
                    Ok((new_tasks, Vec::new()))
                } else {
                    Ok((new_tasks.clone(), vacant.insert(new_tasks).clone()))
                }
            }
        }
    }

    fn record_task_completion(current_package_tasks: Entry<String, Tasks>, task: Task) {
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

    fn launch_task(&self, package_name: &str, task: &Task) -> impl Future<Item = Vec<NewEntry>, Error = ()> {
        let current_tasks_guard = self.current_tasks.clone();
        let pool = self.pool.clone();
        let task = task.clone();
        let package_name = package_name.to_string();

        let mut core = Core::new().unwrap();
        let client = Client::configure()
            .connector(HttpsConnector::new(4, &core.handle()).unwrap())
            .build(&core.handle());

        get_file_stats(client, task.path.clone(), &package_name, task.kind.clone()).then(
            |maybe_stats| {
                let mut current_tasks = current_tasks_guard.lock().unwrap();
                Worker::record_task_completion(current_tasks.entry(package_name), task);

                match maybe_stats {
                    Ok(stats) => {
                        let conn = pool.get().unwrap();
                        let new_entries = stats
                            .iter()
                            .map(|&(ref kind, ref value)| NewEntry {
                                name: &package_name,
                                created: Utc::now().naive_utc(),
                                requested: &task.created,
                                path: &task.path,
                                stat_kind: kind.clone(),
                                file_kind: task.kind.clone(),
                                value: value.clone(),
                                revision: &task.revision,
                            })
                            .collect::<Vec<_>>();
                        diesel::insert_into(entries::table)
                            .values(&new_entries)
                            .execute(&*conn)
                            .unwrap();

                        Ok(new_entries)
                    }
                    Err(err) => {
                        println!(
                            "Error executing task for {}/{}: {:?}",
                            &package_name, &task.path, err
                        );

                        Ok(vec![])
                    }
                }
            },
        )
    }

    pub fn launch_tasks(
        &self,
        name: &str,
        maybe_kind: Option<&FileKind>,
        recursive: bool,
    ) -> Result<(Tasks, Tasks, impl Future<Item = Vec<&NewEntry>>), String> {
        list_files(name, recursive).and_then(|files| {
            let mut current_tasks = self.current_tasks.lock().unwrap();
            let current_package_tasks = current_tasks.entry(name.to_string());

            let new_tasks = files
                .iter()
                .filter_map(|&(ref file, revision)| {
                    get_file_kind(file).and_then(|file_kind| {
                        let requested_kind = maybe_kind.map_or(true, |kind| kind == &file_kind);
                        let in_progress = match current_package_tasks {
                            Entry::Occupied(ref occupied) => occupied.get().iter().any(
                                |&Task {
                                     ref kind, ref path, ..
                                 }| {
                                    kind == &file_kind && path == file
                                },
                            ),
                            _ => false,
                        };
                        if requested_kind && !in_progress {
                            Some(Task {
                                kind: file_kind.clone(),
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

            let future = join_all(
                new_tasks
                    .iter()
                    .map(|task| self.launch_task(name, task))
                    .collect::<Vec<_>>(),
            ).map(|entries| entries.iter().flat_map(|x| x).collect());
            let (new_tasks, in_progress_tasks) =
                Worker::record_new_tasks(current_package_tasks, new_tasks)?;

            Ok((new_tasks, in_progress_tasks, future))
        })
    }

    pub fn get_tasks_in_progress(&self, name: &str) -> Option<Tasks> {
        let current_tasks = self.current_tasks.lock().unwrap();
        current_tasks.get(name).cloned()
    }
}
