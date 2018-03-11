extern crate diesel;
extern crate futures;
extern crate hyper;
extern crate hyper_tls;
extern crate quick_xml;
extern crate tokio_core;

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::str;

use chrono::{NaiveDateTime, Utc};
use self::quick_xml::reader::Reader;
use self::quick_xml::events::Event;
use self::quick_xml::events::attributes::Attribute;
use self::diesel::prelude::*;

use super::models::NewEntry;
use super::schema::entries;
use super::stats::{get_file_kind, get_file_stats, FileKind};

// TODO: add logging

#[derive(Serialize, Clone)]
pub struct Task {
    pub kind: FileKind,
    pub path: String,
    pub revision: i32,
    pub created: NaiveDateTime,
}
type Tasks = Vec<Task>; // TODO: make this a set?

pub struct Worker {
    pool: super::db::Pool,
    current_tasks: Arc<Mutex<HashMap<String, Tasks>>>,
}

fn list_files(name: &str) -> Result<Vec<(String, i32)>, String> {
    let output = Command::new("svn")
        .arg("list")
        .arg("--xml")
        .arg("--recursive")
        .arg(format!("{}/{}/trunk", super::ORGANIZATION_ROOT, name))
        .output();

    match output {
        Ok(Output {
            status, ref stdout, ..
        }) if status.success() =>
        {
            let xml = String::from_utf8_lossy(&stdout);
            let mut reader = Reader::from_str(&xml);
            let mut buf = Vec::new();

            let mut files = Vec::new();
            let mut in_name = false;
            let mut name = String::new();

            loop {
                match reader.read_event(&mut buf) {
                    Ok(Event::Start(ref e)) if e.name() == b"name" => in_name = true,
                    Ok(Event::Start(ref e)) if e.name() == b"commit" => for a in e.attributes() {
                        let Attribute { value, key } = a.unwrap();
                        if str::from_utf8(key).unwrap() == "revision" {
                            files.push((
                                name.clone(),
                                str::from_utf8(&value)
                                    .unwrap()
                                    .parse::<i32>()
                                    .unwrap()
                                    .clone(),
                            ));
                            break;
                        }
                    },
                    Ok(Event::Text(ref e)) if in_name => {
                        name = e.unescape_and_decode(&reader).unwrap()
                    }
                    Ok(Event::End(ref e)) if e.name() == b"name" => in_name = false,
                    Ok(Event::Eof) => break,
                    Err(e) => panic!("Error at position {}: {:?}", reader.buffer_position(), e),
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

    fn launch_task(&self, package_name: &str, task: &Task) {
        let current_tasks_guard = self.current_tasks.clone();
        let pool = self.pool.clone();
        let task = task.clone();
        let package_name = package_name.to_string();

        thread::spawn(move || {
            // TODO: log a failure
            if let Ok(stats) = get_file_stats(&task.path, &package_name, &task.kind) {
                let conn = pool.get().unwrap();
                let new_entries = stats
                    .iter()
                    .map(|&(ref kind, ref value)| NewEntry {
                        name: &package_name,
                        created: Utc::now().naive_utc(),
                        requested: &task.created,
                        path: &task.path,
                        kind: kind.to_string().to_lowercase(),
                        value: value.clone(),
                        revision: &task.revision,
                    })
                    .collect::<Vec<_>>();
                diesel::insert_into(entries::table)
                    .values(&new_entries)
                    .execute(&*conn)
                    .unwrap();
            };

            let mut current_tasks = current_tasks_guard.lock().unwrap();
            if let Entry::Occupied(ref mut occupied) = current_tasks.entry(package_name) {
                if let Some(position) = occupied.get().iter().position(
                    |&Task {
                         ref kind, ref path, ..
                     }| { kind == &task.kind && path == &task.path },
                ) {
                    occupied.get_mut().remove(position); // TODO: clear if empty
                }
            }
        });
    }

    pub fn launch_tasks(
        &self,
        name: &str,
        maybe_kind: Option<&FileKind>,
    ) -> Result<(Tasks, Tasks), String> {
        list_files(name).and_then(|files| {
            let mut current_tasks = self.current_tasks.lock().unwrap();
            let current_package_tasks = current_tasks.entry(name.to_string());

            let new_tasks = files
                .iter()
                .filter_map(|&(ref file, revision)| {
                    get_file_kind(file).and_then(|file_kind| {
                        let requested_kind = maybe_kind.map_or(true, |kind| kind == &file_kind);
                        let in_progress = match current_package_tasks {
                            Entry::Occupied(ref occupied) => occupied
                                .get()
                                .iter()
                                .find(
                                    |&&Task {
                                         ref kind, ref path, ..
                                     }| {
                                        kind == &file_kind && path == file
                                    },
                                )
                                .is_some(),
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

            for task in &new_tasks {
                self.launch_task(name, task);
            }

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
        })
    }

    pub fn get_tasks_in_progress(&self, name: &str) -> Option<Tasks> {
        let current_tasks = self.current_tasks.lock().unwrap();
        current_tasks.get(name).cloned()
    }
}
