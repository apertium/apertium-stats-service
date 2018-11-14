use std::{
    collections::{hash_map::Entry, HashMap},
    process::{Command, Output},
    str,
    sync::{Arc, Mutex},
};

use chrono::{NaiveDateTime, Utc};
use diesel::{self, RunQueryDsl};
use hyper::{client::connect::HttpConnector, Client};
use hyper_tls::HttpsConnector;
use quick_xml::{
    events::{attributes::Attribute, BytesText, Event},
    Reader,
};
use slog::Logger;
use tokio::prelude::{future::join_all, Future};

use db::Pool;
use models::{FileKind, NewEntry};
use schema::entries;
use stats::{get_file_kind, get_file_stats};
use ORGANIZATION_ROOT;

#[derive(Serialize, Clone, Debug)]
pub struct File {
    pub path: String,
    pub size: i32,
    pub revision: i32,
    pub sha: String,
    pub last_author: String,
    pub last_changed: NaiveDateTime,
}

#[derive(Serialize, Clone, Debug)]
pub struct Task {
    pub created: NaiveDateTime,
    pub file: File,
    pub kind: FileKind,
}
type Tasks = Vec<Task>;

pub struct Worker {
    pool: Pool,
    current_tasks: Arc<Mutex<HashMap<String, Tasks>>>,
    logger: Logger,
}

fn list_files(logger: &Logger, name: &str, recursive: bool) -> Result<Vec<File>, String> {
    fn decode_utf8<'a>(bytes: &'a [u8], reader: &Reader<&[u8]>) -> Result<&'a str, String> {
        str::from_utf8(bytes).map_err(|err| {
            format!(
                "UTF8 decoding error at position {}: {:?}",
                reader.buffer_position(),
                err,
            )
        })
    }

    fn decode_bytes(bytes: &BytesText, reader: &Reader<&[u8]>) -> Result<String, String> {
        bytes
            .unescape_and_decode(&reader)
            .map_err(|err| format!("Decoding error at position {}: {:?}", reader.buffer_position(), err,))
    }

    let output = Command::new("svn")
        .arg("list")
        .arg("--xml")
        .args(if recursive { vec!["--recursive"] } else { vec![] })
        .arg(format!("{}/{}/trunk", ORGANIZATION_ROOT, name))
        .output();

    match output {
        Ok(Output { status, ref stdout, .. }) if status.success() => {
            let xml = String::from_utf8_lossy(stdout);
            let mut reader = Reader::from_str(&xml);
            let mut buf = Vec::new();

            let mut files = Vec::new();
            let mut in_file_entry = false;
            let (mut in_name, mut in_author, mut in_date, mut in_size) = (false, false, false, false);
            let (mut name, mut author, mut date, mut size, mut revision) = (None, None, None, None, None);
            loop {
                match reader.read_event(&mut buf) {
                    Ok(Event::Start(ref e)) if e.name() == b"entry" => {
                        in_file_entry = e.attributes().any(|attr| {
                            attr.map(|Attribute { value, key }| {
                                decode_utf8(&key, &reader) == Ok("kind") && decode_utf8(&value, &reader) == Ok("file")
                            }).unwrap_or(false)
                        });
                    },
                    Ok(Event::Start(ref e)) if in_file_entry && e.name() == b"author" => in_author = true,
                    Ok(Event::Start(ref e)) if in_file_entry && e.name() == b"date" => in_date = true,
                    Ok(Event::Start(ref e)) if in_file_entry && e.name() == b"name" => in_name = true,
                    Ok(Event::Start(ref e)) if in_file_entry && e.name() == b"size" => in_size = true,
                    Ok(Event::Start(ref e)) if in_file_entry && e.name() == b"commit" => {
                        for attr in e.attributes() {
                            if let Ok(Attribute { value, key }) = attr {
                                if decode_utf8(&key, &reader)? == "revision" {
                                    revision = Some(decode_utf8(&value, &reader)?.parse::<i32>().map_err(|err| {
                                        format!(
                                            "Revision number parsing error at position {}: {:?}",
                                            reader.buffer_position(),
                                            err,
                                        )
                                    })?);
                                    break;
                                }
                            }
                        }
                    },
                    Ok(Event::Text(ref e)) if in_name => {
                        name = Some(decode_bytes(e, &reader)?);
                    },
                    Ok(Event::Text(ref e)) if in_date => {
                        date = Some(
                            NaiveDateTime::parse_from_str(
                                &decode_bytes(e, &reader)?.to_string(),
                                "%Y-%m-%dT%H:%M:%S.%fZ",
                            ).map_err(|err| {
                                format!(
                                    "Datetime parsing error at position {}: {:?}",
                                    reader.buffer_position(),
                                    err,
                                )
                            })?,
                        );
                    },
                    Ok(Event::Text(ref e)) if in_author => {
                        author = Some(decode_bytes(e, &reader)?);
                    },
                    Ok(Event::Text(ref e)) if in_size => {
                        size = Some(decode_bytes(e, &reader)?.parse::<i32>().map_err(|err| {
                            format!(
                                "File size number parsing error at position {}: {:?}",
                                reader.buffer_position(),
                                err,
                            )
                        })?);
                    },
                    Ok(Event::End(ref e)) if e.name() == b"author" => in_author = false,
                    Ok(Event::End(ref e)) if e.name() == b"date" => in_date = false,
                    Ok(Event::End(ref e)) if e.name() == b"name" => in_name = false,
                    Ok(Event::End(ref e)) if e.name() == b"size" => in_size = false,
                    Ok(Event::End(ref e)) if e.name() == b"entry" => {
                        if in_file_entry {
                            match (name.clone(), size, revision, author.clone(), date) {
                                (Some(name), Some(size), Some(revision), Some(author), Some(date)) => {
                                    let sha_output = Command::new("svn")
                                        .arg("propget")
                                        .arg("git-commit")
                                        .arg("--revprop")
                                        .arg("-r")
                                        .arg("HEAD")
                                        .arg(format!("{}/{}", ORGANIZATION_ROOT, name))
                                        .output();
                                    let sha = match sha_output {
                                        Ok(Output { status, ref stdout, .. }) if status.success() => {
                                            Ok(String::from_utf8_lossy(stdout))
                                        }
                                    };
                                    let sha_string = sha.unwrap().to_string();
                                    trace!(
                                        logger,
                                        "Parsed file";
                                        "name" => name.clone(), "size" => size, "revision" => revision, "author" => author.clone(), "date" => date.to_string(),
                                    );
                                    files.push(File {
                                        path: name,
                                        size,
                                        revision,
                                        last_author: author,
                                        last_changed: date,
                                        sha: sha_string
                                    });
                                },
                                _ => {
                                    warn!(
                                        logger,
                                        "Failed to fetch all file information";
                                        "name" => name, "size" => size, "revision" => revision, "author" => author, "date" => date.map(|x| x.to_string())
                                    );
                                },
                            }
                        }

                        in_file_entry = false;
                        size = None;
                        revision = None;
                        author = None;
                        date = None;
                    },
                    Ok(Event::Eof) => break,
                    Err(err) => panic!("Error at position {}: {:?}", reader.buffer_position(), err),
                    _ => (),
                }
                buf.clear();
            }

            Ok(files)
        },
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

        list_files(&logger, name, recursive).and_then(|files| {
            let mut current_tasks = self.current_tasks.lock().unwrap();
            let current_package_tasks = current_tasks.entry(name.to_string());

            let new_tasks = files
                .into_iter()
                .filter_map(|file| {
                    get_file_kind(&file.path).and_then(|file_kind| {
                        let requested_kind = maybe_kind.map_or(true, |kind| kind == &file_kind);
                        let in_progress = match current_package_tasks {
                            Entry::Occupied(ref occupied) => occupied.get().into_iter().any(
                                |Task {
                                     kind,
                                     file: File { path, .. },
                                     ..
                                 }| { kind == &file_kind && path == &file.path },
                            ),
                            _ => false,
                        };
                        if requested_kind && !in_progress {
                            Some(Task {
                                kind: file_kind,
                                file,
                                created: Utc::now().naive_utc(),
                            })
                        } else {
                            None
                        }
                    })
                }).collect::<Vec<_>>();
            info!(logger, "Spawning {} task(s): {:?}", new_tasks.len(), new_tasks,);

            let future = join_all(
                new_tasks
                    .iter()
                    .map(|task| self.launch_task(&logger, client, name, task))
                    .collect::<Vec<_>>(),
            ).map(|entries| entries.into_iter().flat_map(|x| x).collect());
            let (new_tasks, in_progress_tasks) = Worker::record_new_tasks(current_package_tasks, new_tasks)?;

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
            "path" => task.file.path.clone(),
            "kind" => task.kind.to_string(),
        ));

        get_file_stats(
            &logger,
            &client,
            task.file.path.clone(),
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
                            path: task.file.path.clone(),
                            stat_kind: kind,
                            file_kind: task.kind.clone(),
                            value: value.into(),
                            revision: task.file.revision,
                            sha: task.file.sha,
                            size: task.file.size,
                            last_author: task.file.last_author.clone(),
                            last_changed: task.file.last_changed,
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
            if let Some(position) = occupied
                .get()
                .iter()
                .position(|&Task { ref kind, ref file, .. }| kind == &task.kind && file.path == task.file.path)
            {
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
