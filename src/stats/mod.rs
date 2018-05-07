mod lexc;
mod xml;

use std::{
    io::{self, BufRead, BufReader, Write}, process::{Command, Output}, str,
};

use hyper::{client::connect::HttpConnector, Client, Error as HyperError};
use hyper_tls::HttpsConnector;
use regex::{Regex, RegexSet, RegexSetBuilder};
use serde_json::Value;
use slog::Logger;
use tempfile::NamedTempFile;
use tokio::prelude::{Future, Stream};

use models::{FileKind, StatKind};
use LANG_CODE_RE;
use ORGANIZATION_RAW_ROOT;

#[derive(Debug)]
pub enum StatsError {
    Hyper(HyperError),
    Utf8(str::Utf8Error),
    Io(io::Error),
    Xml(String),
    CgComp(String),
    Lexc(String),
}

pub fn get_file_stats(
    logger: &Logger,
    client: &Client<HttpsConnector<HttpConnector>>,
    file_path: String,
    package_name: &str,
    file_kind: FileKind,
) -> impl Future<Item = Vec<(StatKind, Value)>, Error = StatsError> {
    let url = format!("{}/{}/master/{}", ORGANIZATION_RAW_ROOT, package_name, file_path)
        .parse()
        .unwrap();
    let logger = logger.clone();

    client.get(url).map_err(StatsError::Hyper).and_then(|response| {
        response
            .into_body()
            .concat2()
            .map_err(StatsError::Hyper)
            .and_then(move |body| match file_kind {
                FileKind::Monodix | FileKind::MetaMonodix => self::xml::get_monodix_stats(body, &file_path),
                FileKind::Bidix | FileKind::MetaBidix | FileKind::Postdix => {
                    self::xml::get_bidix_stats(body, &file_path)
                },
                FileKind::Transfer => self::xml::get_transfer_stats(body, &file_path),
                FileKind::Rlx => {
                    let mut rlx_file = NamedTempFile::new().map_err(StatsError::Io)?;
                    rlx_file.write_all(&*body).map_err(StatsError::Io)?;
                    let output = Command::new("cg-comp")
                        .arg(rlx_file
                            .path()
                            .to_str()
                            .ok_or_else(|| StatsError::CgComp("Unable to create temporary file".to_string()))?)
                        .arg("/dev/null")
                        .output();

                    match output {
                        Ok(Output { status, ref stderr, .. }) if status.success() => {
                            let cg_conv_output = String::from_utf8_lossy(stderr);
                            lazy_static! {
                                static ref RE: Regex = Regex::new(r"(\w+): (\d+)").unwrap();
                            }
                            for capture in RE.captures_iter(&cg_conv_output) {
                                if &capture[1] == "Rules" {
                                    return Ok(vec![(StatKind::Rules, json!(capture[2]))]);
                                }
                            }

                            Err(StatsError::CgComp(format!("No stats in output: {}", &cg_conv_output)))
                        },
                        Ok(Output { ref stderr, .. }) => {
                            Err(StatsError::CgComp(String::from_utf8_lossy(stderr).to_string()))
                        },
                        Err(err) => Err(StatsError::Io(err)),
                    }
                },
                FileKind::Twol => {
                    let rule_count = BufReader::new(&*body)
                        .lines()
                        .filter(|line_result| line_result.as_ref().ok().map_or(false, |line| line.starts_with('"')))
                        .count();
                    Ok(vec![(StatKind::Rules, json!(rule_count))])
                },
                FileKind::Lexc => self::lexc::get_stats(&logger, body),
            })
    })
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
            ]).size_limit(50_000_000)
                .build()
                .unwrap()
        };
    }

    let matches = RE.matches(file_name.trim_right_matches(".xml"));
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
        _ => None,
    })
}
