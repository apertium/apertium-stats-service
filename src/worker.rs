use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    process::{Command, Output},
    str,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use chrono::{NaiveDateTime, Utc};
use diesel::{self, RunQueryDsl};
use failure::Fail;
use graphql_client::{GraphQLQuery, Response};
use lazy_static::lazy_static;
use quick_xml::{
    events::{attributes::Attribute, BytesText, Event},
    Reader,
};
use serde_derive::Serialize;
use slog::{debug, error, info, o, trace, warn, Logger};
use tokio::{
    executor::current_thread::CurrentThread,
    prelude::{future::join_all, Future},
};
use tokio_process::CommandExt;

use crate::{
    db::Pool,
    models::{FileKind, NewEntry},
    schema::entries,
    stats::{get_file_kind, get_file_stats},
    GITHUB_GRAPHQL_API_ENDPOINT, HTTPS_CLIENT, ORGANIZATION_ROOT,
};

type DateTime = chrono::DateTime<Utc>;
type GitObjectID = String;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/schema.json",
    query_path = "src/graphql/packages_query.graphql",
    response_derives = "Clone, Debug"
)]
pub struct PackagesQuery;

#[derive(Clone, Debug, Serialize)]
pub struct File {
    pub path: String,
    pub size: i32,
    pub revision: i32,
    pub sha: String,
    pub last_author: String,
    pub last_changed: NaiveDateTime,
}

pub struct FileWithoutSha {
    pub path: String,
    pub size: i32,
    pub revision: i32,
    pub last_author: String,
    pub last_changed: NaiveDateTime,
}

#[derive(Clone, Debug, Serialize)]
pub struct Task {
    pub created: NaiveDateTime,
    pub file: File,
    pub kind: FileKind,
}
type Tasks = Vec<Task>;

#[derive(Clone, Serialize)]
pub struct Actor {
    pub name: String,
    pub email: String,
}

#[derive(Clone, Serialize)]
pub struct Commit {
    pub sha: String,
    pub author: Actor,
    pub committer: Actor,
    pub authored: NaiveDateTime,
    pub committed: NaiveDateTime,
    pub message: String,
}

#[derive(Clone, Serialize)]
pub struct Package {
    pub name: String,
    pub description: Option<String>,
    pub topics: Vec<String>,
    pub last_commit: Option<Commit>,
}

#[derive(Debug, Fail)]
enum PackageUpdateError {
    #[fail(display = "Missing response data key: {}", _0)]
    MissingData(String),
}

fn get_git_sha(logger: Logger, revision: i32, svn_path: &str) -> impl Future<Item = Option<String>, Error = ()> {
    let sha_future = Command::new("svn")
        .arg("propget")
        .arg("git-commit")
        .arg("--revprop")
        .arg("-r")
        .arg(revision.to_string())
        .arg(svn_path)
        .output_async();

    sha_future.then(move |sha| match sha {
        Ok(Output { status, ref stdout, .. }) if status.success() => {
            let sha = String::from_utf8_lossy(stdout).into_owned().as_str().trim().to_string();
            Ok(Some(sha))
        },
        Ok(Output { stderr, .. }) => {
            let err = String::from_utf8_lossy(&stderr);
            error!(logger, "Error getting SHA corresponding to revision: {:?}", err; "revision" => revision);
            Ok(None)
        },
        Err(err) => {
            error!(logger, "Error getting SHA corresponding to revision: {:?}", err; "revision" => revision);
            Ok(None)
        },
    })
}

#[allow(clippy::cognitive_complexity)]
fn list_files(logger: &Logger, package_name: &str, recursive: bool) -> Result<Vec<FileWithoutSha>, String> {
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

    let svn_path = format!("{}/{}/trunk", ORGANIZATION_ROOT, package_name);
    let output = Command::new("svn")
        .arg("list")
        .arg("--xml")
        .args(if recursive { vec!["--recursive"] } else { vec![] })
        .arg(&svn_path)
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
                            })
                            .unwrap_or(false)
                        });
                    },
                    Ok(Event::Start(ref e)) if in_file_entry && e.name() == b"author" => in_author = true,
                    Ok(Event::Start(ref e)) if in_file_entry && e.name() == b"date" => in_date = true,
                    Ok(Event::Start(ref e)) if in_file_entry && e.name() == b"name" => in_name = true,
                    Ok(Event::Start(ref e)) if in_file_entry && e.name() == b"size" => in_size = true,
                    Ok(Event::Start(ref e)) if in_file_entry && e.name() == b"commit" => {
                        for Attribute { value, key } in e.attributes().flatten() {
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
                    },
                    Ok(Event::Text(ref e)) if in_name => {
                        name = Some(decode_bytes(e, &reader)?);
                    },
                    Ok(Event::Text(ref e)) if in_date => {
                        date = Some(
                            NaiveDateTime::parse_from_str(
                                &decode_bytes(e, &reader)?.to_string(),
                                "%Y-%m-%dT%H:%M:%S.%fZ",
                            )
                            .map_err(|err| {
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
                                    trace!(
                                        logger,
                                        "Parsed file";
                                        "name" => name.clone(), "size" => size, "revision" => revision, "author" => author.clone(), "date" => date.to_string(),
                                    );
                                    files.push(FileWithoutSha {
                                        path: name,
                                        size,
                                        revision,
                                        last_author: author,
                                        last_changed: date,
                                    });
                                },
                                _ => {
                                    error!(
                                        logger,
                                        "Failed to fetch all file information";
                                        "name" => name, "size" => size, "revision" => revision, "author" => author, "date" => date.map(|x| x.to_string()),
                                    );
                                },
                            }
                        }

                        in_file_entry = false;
                        name = None;
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
        Err(err) => Err(format!("Package search failed: {}", err)),
    }
}

fn get_packages(
    logger: &Logger,
    github_auth_token: &str,
    after: Option<&String>,
) -> Result<(Vec<Package>, Option<String>, packages_query::PackagesQueryRateLimit), failure::Error> {
    lazy_static! {
        static ref TOPICS: HashSet<&'static str> = [
            "apertium-languages",
            "apertium-incubator",
            "apertium-nursery",
            "apertium-staging",
            "apertium-trunk"
        ]
        .iter()
        .cloned()
        .collect();
    }

    debug!(logger, "Fetching repos"; "after" => after);

    let query = PackagesQuery::build_query(packages_query::Variables { after: after.cloned() });
    let response: Response<packages_query::ResponseData> = HTTPS_CLIENT
        .post(GITHUB_GRAPHQL_API_ENDPOINT)
        .bearer_auth(github_auth_token)
        .json(&query)
        .send()?
        .error_for_status()?
        .json()?;

    if let Some(errors) = response.errors {
        for err in &errors {
            warn!(logger, "Encountered error while fetching repos: {:?}", err; "after" => after);
        }
    }

    let response_data: packages_query::ResponseData = response
        .data
        .ok_or_else(|| PackageUpdateError::MissingData("data".to_string()))?;

    let repositories = response_data
        .organization
        .ok_or_else(|| PackageUpdateError::MissingData("data.organization".to_string()))?
        .repositories;

    let packages = repositories
        .edges
        .ok_or_else(|| PackageUpdateError::MissingData("data.organization.repository.edges".to_string()))?
        .into_iter()
        .filter_map(|repo_node| {
            if repo_node.is_none() {
                warn!(logger, "Missing repository node");
            }

            repo_node.and_then(|x| x.node).map(|repo| Package {
                name: repo.name,
                description: repo.description,
                topics: repo.repository_topics.nodes.map_or(vec![], |topics| {
                    topics
                        .into_iter()
                        .filter_map(|topic| topic.map(|x| x.topic).map(|x| x.name))
                        .collect()
                }),
                last_commit: repo.ref_.and_then(|x| match x.target.on {
                    packages_query::PackagesQueryOrganizationRepositoriesEdgesNodeRefTargetOn::Commit(commit) => commit
                        .history
                        .edges
                        .and_then(|x| x.first().and_then(|y| y.clone().and_then(|z| z.node)))
                        .and_then(|ref commit| match (&commit.author, &commit.committer) {
                            (Some(ref author), Some(ref committer)) =>
                                match (&author.name, &author.email, &committer.name, &committer.email) {
                                    (Some(ref author_name), Some(ref author_email), Some(ref committer_name), Some(ref committer_email)) =>
                                        Some(Commit {
                                            sha: commit.oid.clone(),
                                            message: commit.clone().message_headline,
                                            authored: commit.authored_date.naive_utc(),
                                            committed: commit.committed_date.naive_utc(),
                                            author: Actor {
                                                name: author_name.to_string(),
                                                email: author_email.to_string(),
                                            },
                                            committer: Actor {
                                                name: committer_name.to_string(),
                                                email: committer_email.to_string(),
                                            },
                                        }),
                                    _ => {
                                        warn!(logger, "Commit author or committer missing information: author = {:?}, committer = {:?}", author, committer);
                                        None
                                    }
                                }
                            _ => {
                                warn!(logger, "Commit missing author or committer: {:?}", commit);
                                None
                            },
                        }),
                    _ => None,
                }),
            })
        })
        .filter(|Package {topics, ..}| !TOPICS.is_disjoint(&topics.iter().map(|x| x.as_str()).collect()))
        .collect::<Vec<_>>();

    let page_info = repositories.page_info;
    let next_after = if page_info.has_next_page {
        page_info.end_cursor
    } else {
        None
    };

    let limits = response_data
        .rate_limit
        .ok_or_else(|| PackageUpdateError::MissingData("data.organization.rate_limit".to_string()))?;

    debug!(logger, "Fetched {} packages", packages.len());

    Ok((packages, next_after, limits))
}

pub struct Worker {
    pub logger: Logger,
    pub packages: RwLock<Vec<Package>>,
    pub packages_updated: RwLock<Option<NaiveDateTime>>,
    pub packages_next_update: RwLock<NaiveDateTime>,
    packages_update_mutex: Mutex<()>,
    pool: Pool,
    current_tasks: Arc<RwLock<HashMap<String, Tasks>>>,
    github_auth_token: Option<String>,
}

impl Worker {
    pub fn new(pool: Pool, logger: Logger, github_auth_token: Option<String>) -> Worker {
        Worker {
            pool,
            packages: RwLock::new(vec![]),
            packages_updated: RwLock::new(None),
            packages_next_update: RwLock::new(Utc::now().naive_utc()),
            packages_update_mutex: Mutex::new(()),
            current_tasks: Arc::new(RwLock::new(HashMap::new())),
            logger,
            github_auth_token,
        }
    }

    pub fn get_tasks_in_progress(&self, name: &str) -> Option<Tasks> {
        let current_tasks = self.current_tasks.read().unwrap();
        current_tasks.get(name).cloned()
    }

    pub fn launch_tasks(
        &self,
        name: &str,
        maybe_kind: Option<&FileKind>,
        recursive: bool,
    ) -> Result<(Tasks, Tasks, impl Future<Item = Vec<NewEntry>>), String> {
        let logger = self.logger.new(o!(
            "package" => name.to_string(),
            "recursive" => recursive,
        ));

        list_files(&logger, name, recursive).and_then(|files_without_shas| {
            let mut current_tasks = self.current_tasks.write().unwrap();
            let current_package_tasks = current_tasks.entry(name.to_string());

            let requested_files = files_without_shas
                .into_iter()
                .filter_map(|file| {
                    get_file_kind(&file.path).and_then(|file_kind| {
                        let requested_kind = maybe_kind.map_or(true, |kind| kind == &file_kind);
                        let in_progress = match current_package_tasks {
                            Entry::Occupied(ref occupied) => occupied.get().iter().any(
                                |Task {
                                     kind,
                                     file: File { path, .. },
                                     ..
                                 }| { kind == &file_kind && path == &file.path },
                            ),
                            _ => false,
                        };
                        if requested_kind && !in_progress {
                            Some((file_kind, file))
                        } else {
                            None
                        }
                    })
                })
                .collect::<Vec<_>>();

            let svn_path = format!("{}/{}/trunk", ORGANIZATION_ROOT, name);
            let mut unique_revisions = requested_files
                .iter()
                .map(|(_, FileWithoutSha { revision, .. })| *revision)
                .collect::<Vec<_>>();
            unique_revisions.sort_unstable();
            unique_revisions.dedup();
            debug!(logger, "Found {} unique revisions", unique_revisions.len());

            let sha_futures = join_all(
                unique_revisions
                    .iter()
                    .map(|&revision| get_git_sha(logger.clone(), revision, &svn_path))
                    .collect::<Vec<_>>(),
            );
            let new_tasks = match CurrentThread::new().block_on(sha_futures) {
                Ok(shas) => {
                    let revision_sha_mapping = unique_revisions
                        .into_iter()
                        .zip(shas)
                        .collect::<HashMap<i32, Option<String>>>();
                    debug!(
                        logger,
                        "Fetched Git SHAs for {} unique revisions",
                        revision_sha_mapping.len()
                    );

                    let tasks = requested_files
                        .into_iter()
                        .filter_map(
                            |(file_kind, FileWithoutSha {
                                path,
                                size,
                                revision,
                                last_author,
                                last_changed,
                            })| match revision_sha_mapping.get(&revision) {
                                Some(Some(sha)) => Some(Task {
                                    kind: file_kind,
                                    file: File {
                                        path,
                                        size,
                                        revision,
                                        last_author,
                                        last_changed,
                                        sha: sha.to_string(),
                                    },
                                    created: Utc::now().naive_utc(),
                                }),
                                _ => {
                                    error!(logger, "Missing SHA corresponding to file"; "path" => path, "revision" => revision);
                                    None
                                },
                            },
                        )
                        .collect::<Vec<_>>();
                    Ok(tasks)
                },
                Err(err) => Err(format!("Unable to fetch Git SHAs: {}", err)),
            }?;

            info!(logger, "Spawning {} task(s): {:?}", new_tasks.len(), new_tasks);
            let future = join_all(
                new_tasks
                    .iter()
                    .map(|task| self.launch_task(&logger, name, task))
                    .collect::<Vec<_>>(),
            )
            .map(|entries| {
                entries
                    .into_iter()
                    .flat_map(|x| x.0.unwrap_or_else(Vec::new))
                    .collect()
            });
            let (new_tasks, in_progress_tasks) = Worker::record_new_tasks(current_package_tasks, new_tasks)?;

            Ok((new_tasks, in_progress_tasks, future))
        })
    }

    pub fn update_packages(&self) -> Result<Duration, failure::Error> {
        let _guard = self.packages_update_mutex.lock().unwrap();
        let github_auth_token = self
            .github_auth_token
            .as_ref()
            .expect("package list update requires a GitHub auth token")
            .as_str();
        let mut packages = Vec::new();

        let (mut new_packages, mut after, mut rate_limits) = get_packages(&self.logger, github_auth_token, None)?;
        let mut total_cost = rate_limits.cost;
        packages.append(&mut new_packages);
        while after.is_some() {
            let (mut new_packages, new_after, new_rate_limits) =
                get_packages(&self.logger, github_auth_token, after.as_ref())?;
            after = new_after;
            rate_limits = new_rate_limits;
            total_cost += rate_limits.cost;
            packages.append(&mut new_packages);
        }

        let mut packages_lock = self.packages.write().unwrap();
        packages_lock.clear();
        packages_lock.append(&mut packages);
        *self.packages_updated.write().unwrap() = Some(Utc::now().naive_utc());

        let next_update = (rate_limits.reset_at - Utc::now()) / ((rate_limits.remaining / total_cost) as i32);
        info!(
            self.logger,
            "Completed package list update";
            "length" => packages_lock.len(), "total_cost" => total_cost,
            "cost_remaining" => rate_limits.remaining, "next_update_min" => next_update.to_string()
        );
        Ok(chrono::Duration::to_std(&next_update)?)
    }

    pub fn record_next_packages_update(&self, next_update: Duration) {
        debug!(self.logger, "Next package update in {:?}", next_update);
        *self.packages_next_update.write().unwrap() = Utc::now().naive_utc()
            + chrono::Duration::from_std(next_update).unwrap_or_else(|_| chrono::Duration::zero());
    }

    fn launch_task(
        &self,
        logger: &Logger,
        package_name: &str,
        task: &Task,
    ) -> impl Future<Item = (Option<Vec<NewEntry>>, Option<String>), Error = ()> {
        let current_tasks_guard = self.current_tasks.clone();
        let pool = self.pool.clone();
        let task = task.clone();
        let package_name = package_name.to_string();
        let logger = logger.new(o!(
            "path" => task.file.path.clone(),
            "kind" => task.kind.to_string(),
        ));

        get_file_stats(&logger, task.file.path.clone(), &package_name, task.kind.clone()).then(move |maybe_stats| {
            let mut current_tasks = current_tasks_guard.write().unwrap();
            Worker::record_task_completion(current_tasks.entry(package_name.clone()), &task);

            match maybe_stats {
                Ok(stats) => {
                    debug!(logger, "Completed executing task");

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
                            sha: task.file.sha.clone(),
                            size: task.file.size,
                            last_author: task.file.last_author.clone(),
                            last_changed: task.file.last_changed,
                        })
                        .collect::<Vec<_>>();

                    match pool.get() {
                        Ok(conn) => {
                            diesel::insert_into(entries::table)
                                .values(&new_entries)
                                .execute(&*conn)
                                .unwrap();
                            Ok((Some(new_entries), None))
                        },
                        Err(err) => {
                            error!(logger, "Error persisting task results: {:?}", err);
                            Ok((
                                Some(new_entries),
                                Some(format!("Error persisting task results: {:?}", err)),
                            ))
                        },
                    }
                },
                Err(err) => {
                    error!(logger, "Error executing task: {:?}", err);
                    Ok((None, Some(format!("Error executing task: {:?}", err))))
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
