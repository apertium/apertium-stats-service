extern crate diesel;
extern crate futures;
extern crate hyper;
extern crate hyper_tls;
extern crate quick_xml;
extern crate tokio_core;

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::process::{Command, Output};
use std::sync::Mutex;
use std::thread;
use std::str;

use chrono::{NaiveDateTime, Utc};
use regex::RegexSet;
use self::futures::{Future, Stream};
use self::hyper::Client;
use self::tokio_core::reactor::Core;
use self::hyper_tls::HttpsConnector;
use self::quick_xml::reader::Reader;
use self::quick_xml::events::Event;
use self::diesel::prelude::*;

use super::models::NewEntry;
use super::schema::entries;

// TODO: add logging

#[derive(Serialize, Clone)]
pub struct Task {
    // TODO: make this full of refs?
    pub kind: String,
    pub path: String,
    pub revision: i32,
    pub created: NaiveDateTime,
}
type Tasks = Vec<Task>; // TODO: make this a set?

pub struct Worker {
    pool: super::db::Pool,
    current_tasks: Mutex<HashMap<String, Tasks>>, // TODO: make this full of refs instead?
}

fn get_file_kind(file_name: &str) -> Option<&str> {
    lazy_static! {
        static ref RE: RegexSet = RegexSet::new(&[
            format!(r"^apertium-({re})-({re})\.({re})-({re})\.dix$", re=super::LANG_CODE_RE),
        ]).unwrap();
    }

    let matches = RE.matches(file_name);
    if matches.matched(0) {
        Some("bidix") // TODO: change this to enum
    } else {
        // TODO: implement the rest
        None
    }
}

impl Worker {
    pub fn new(pool: super::db::Pool) -> Worker {
        Worker {
            pool,
            current_tasks: Mutex::new(HashMap::new()),
        }
    }

    fn launch_task(&self, package_name: &str, task: &Task) {
        // TODO: handle removal of task on failure
        let url = format!(
            "{}/{}/master/{}",
            super::ORGANIZATION_RAW_ROOT,
            package_name,
            task.path
        ).parse()
            .unwrap();
        let pool = self.pool.clone();
        let task = task.clone();
        let package_name = package_name.to_string();

        thread::spawn(move || {
            let mut core = Core::new().unwrap(); // TODO: make either of these instance variables (or static?)
            let client = Client::configure()
                .connector(HttpsConnector::new(4, &core.handle()).unwrap())
                .build(&core.handle());
            let work = client.get(url).and_then(|response| {
                response.body().concat2().and_then(move |body| {
                    let mut reader = Reader::from_str(&str::from_utf8(&*body).unwrap());

                    let mut count = 0;
                    let mut buf = Vec::new();
                    let mut in_section = false;

                    // TODO: match actions on task.kind
                    loop {
                        match reader.read_event(&mut buf) {
                            Ok(Event::Start(ref e)) if e.name() == b"section" => in_section = true,
                            Ok(Event::Start(ref e)) if in_section && e.name() == b"e" => count += 1,
                            Ok(Event::End(ref e)) if e.name() == b"section" => in_section = false,
                            Ok(Event::Eof) => break,
                            Err(e) => {
                                panic!("Error at position {}: {:?}", reader.buffer_position(), e)
                            }
                            _ => (),
                        }
                        buf.clear();
                    }

                    let conn = pool.get().unwrap();
                    let new_entry = NewEntry {
                        name: &package_name,
                        created: &Utc::now().naive_utc(),
                        requested: &task.created,
                        path: &task.path,
                        kind: &task.kind,
                        value: &count.to_string(),
                        revision: &task.revision,
                    };
                    diesel::insert_into(entries::table)
                        .values(&new_entry)
                        .execute(&*conn)
                        .unwrap();

                    Ok(())
                })
            });
            core.run(work).unwrap();
        });
    }

    pub fn launch_tasks(
        &self,
        name: &str,
        maybe_kind: Option<&str>,
    ) -> Result<(Tasks, Tasks), String> {
        let output = Command::new("svn")
            .arg("list")
            .arg("-R")
            .arg(format!("{}/{}/trunk", super::ORGANIZATION_ROOT, name))
            .output();

        match output {
            Ok(Output {
                status, ref stdout, ..
            }) if status.success() =>
            {
                let mut current_tasks = self.current_tasks.lock().unwrap();
                let current_package_tasks = current_tasks.entry(name.to_string());
                let new_tasks = String::from_utf8_lossy(&stdout) // TODO: get revision from here and use --xml
                    .split("\n")
                    .filter_map(|file| {
                        get_file_kind(file).and_then(|file_kind| {
                            let requested_kind = maybe_kind.map_or(true, |kind| kind == file_kind);
                            let in_progress = match current_package_tasks {
                                Entry::Occupied(ref occupied) => occupied
                                    .get()
                                    .iter()
                                    .find(
                                        |&&Task {
                                             ref kind, ref path, ..
                                         }| {
                                            kind == file_kind && path == file
                                        },
                                    )
                                    .is_some(),
                                _ => false,
                            };
                            if requested_kind && !in_progress {
                                Some(Task {
                                    kind: file_kind.to_string(),
                                    path: file.to_string(),
                                    created: Utc::now().naive_utc(),
                                    revision: 0, // TODO: get the correct one
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
            }
            Ok(Output { stderr, .. }) => {
                let error = String::from_utf8_lossy(&stderr);
                Err(format!("Package not found: {}", error))
            }
            Err(_) => Err(format!("Package search failed: {}", name)),
        }
    }

    pub fn get_tasks_in_progress(&self, name: &str) -> Option<Tasks> {
        let current_tasks = self.current_tasks.lock().unwrap();
        current_tasks.get(name).cloned()
    }
}
