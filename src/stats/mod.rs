mod lexc;
mod lexd;
mod xml;

use std::{
    io::{self, Write},
    num::ParseIntError,
    process::Output,
    str::Utf8Error,
};

use lazy_static::lazy_static;
use regex::{Regex, RegexSet, RegexSetBuilder};
use reqwest::Error as ReqwestError;
use rocket_contrib::{json, json::JsonValue};
use slog::Logger;
use tempfile::NamedTempFile;
use tokio::process::Command;

use crate::{
    models::{FileKind, StatKind},
    util::LANG_CODE_RE,
    HTTPS_CLIENT, ORGANIZATION_RAW_ROOT,
};

#[derive(Debug)]
pub enum StatsError {
    Reqwest(ReqwestError),
    Utf8(Utf8Error),
    Io(io::Error),
    Xml(String),
    CgComp(String),
    Lexd(String),
    Lexc(String),
}

pub type StatsResults = Result<Vec<(StatKind, JsonValue)>, StatsError>;

pub async fn get_file_stats(
    logger: Logger,
    file_path: String,
    package_name: String,
    file_kind: FileKind,
) -> StatsResults {
    let url = format!("{}/{}/master/{}", ORGANIZATION_RAW_ROOT, package_name, file_path);
    let logger = logger.clone();

    let body = HTTPS_CLIENT
        .get(&url)
        .send()
        .await
        .map_err(StatsError::Reqwest)?
        .error_for_status()
        .map_err(StatsError::Reqwest)?
        .text()
        .await
        .map_err(StatsError::Reqwest)?;

    match file_kind {
        FileKind::Monodix | FileKind::MetaMonodix => self::xml::get_monodix_stats(&body, &file_path),
        FileKind::Bidix | FileKind::MetaBidix | FileKind::Postdix => self::xml::get_bidix_stats(&body, &file_path),
        FileKind::Transfer => self::xml::get_transfer_stats(&body, &file_path),
        FileKind::Rlx => {
            let mut rlx_file = NamedTempFile::new().map_err(StatsError::Io)?;
            rlx_file.write_all(body.as_bytes()).map_err(StatsError::Io)?;
            let output = Command::new("cg-comp")
                .arg(
                    rlx_file
                        .path()
                        .to_str()
                        .ok_or_else(|| StatsError::CgComp("Unable to create temporary file".to_string()))?,
                )
                .arg("/dev/null")
                .output()
                .await;

            match output {
                Ok(Output { status, ref stderr, .. }) if status.success() => {
                    let cg_conv_output = String::from_utf8_lossy(stderr);
                    lazy_static! {
                        static ref RE: Regex = Regex::new(r"(\w+): (\d+)").unwrap();
                    }
                    for capture in RE.captures_iter(&cg_conv_output) {
                        if &capture[1] == "Rules" {
                            let rule_count_string = &capture[2];
                            let rule_count: u32 = rule_count_string
                                .parse()
                                .map_err(|e: ParseIntError| StatsError::CgComp(e.to_string()))?;
                            return Ok(vec![(StatKind::Rules, json!(rule_count))]);
                        }
                    }

                    Err(StatsError::CgComp(format!("No stats in output: {}", &cg_conv_output)))
                },
                Ok(Output { ref stderr, .. }) => Err(StatsError::CgComp(String::from_utf8_lossy(stderr).to_string())),
                Err(err) => Err(StatsError::Io(err)),
            }
        },
        FileKind::Twol => {
            let rule_count = body.lines().filter(|line| line.starts_with('"')).count();
            Ok(vec![(StatKind::Rules, json!(rule_count))])
        },
        FileKind::Lexc => self::lexc::get_stats(&logger, &body),
        FileKind::Lexd => self::lexd::get_stats(&logger, &body),
    }
}

pub fn get_file_kind(file_name: &str) -> Option<FileKind> {
    lazy_static! {
        static ref RE: RegexSet = {
            let re = LANG_CODE_RE;
            RegexSetBuilder::new(&[
                format!(r"apertium-{re}\.{re}\.dix$", re = re),
                format!(r"apertium-{re}-{re}\.{re}-{re}\.dix$", re = re),
                format!(r"apertium-{re}\.{re}\.metadix$", re = re),
                format!(r"apertium-{re}-{re}\.{re}\.metadix$", re = re),
                format!(r"apertium-{re}-{re}\.{re}-{re}\.metadix$", re = re),
                format!(r"apertium-{re}-{re}\.post-{re}\.dix$", re = re),
                format!(r"apertium-{re}\.post-{re}\.dix$", re = re),
                format!(r"apertium-{re}-{re}\.{re}-{re}\.rlx$", re = re),
                format!(r"apertium-{re}\.{re}\.rlx$", re = re),
                format!(r"apertium-{re}-{re}\.{re}-{re}\.t\dx$", re = re),
                format!(r"apertium-{re}\.{re}\.lexc$", re = re),
                format!(r"apertium-{re}-{re}\.{re}\.twol$", re = re),
                format!(r"apertium-{re}\.{re}\.twol$", re = re),
                format!(r"apertium-{re}\.{re}\.lexd$", re = re),
            ])
            .size_limit(50_000_000)
            .build()
            .unwrap()
        };
    }

    let matches = RE.matches(file_name.trim_end_matches(".xml"));
    matches.into_iter().collect::<Vec<_>>().pop().and_then(|i| match i {
        0 => Some(FileKind::Monodix),
        1 => Some(FileKind::Bidix),
        2 | 3 => Some(FileKind::MetaMonodix),
        4 => Some(FileKind::MetaBidix),
        5 | 6 => Some(FileKind::Postdix),
        7 | 8 => Some(FileKind::Rlx),
        9 => Some(FileKind::Transfer),
        10 => Some(FileKind::Lexc),
        11 | 12 => Some(FileKind::Twol),
        13 => Some(FileKind::Lexd),
        _ => None,
    })
}
