extern crate futures;
extern crate hyper;
extern crate hyper_tls;
extern crate quick_xml;
extern crate tempfile;
extern crate tokio_core;

use regex::{Regex, RegexSet, RegexSetBuilder};
use std::fmt;
use std::io::{self, Write};
use std::process::{Command, Output};
use std::str;

use self::futures::{Future, Stream};
use self::hyper::Client;
use self::tokio_core::reactor::Core;
use self::hyper_tls::HttpsConnector;
use self::tempfile::NamedTempFile;
use self::quick_xml::reader::Reader;
use self::quick_xml::events::Event;

#[allow(dead_code)]
#[derive(PartialEq, Clone, Debug)]
pub enum StatKind {
    Stems,
    Paradigms,
    Rules,
    Macros,
}

impl fmt::Display for StatKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[allow(dead_code)]
#[derive(PartialEq, Clone, Debug, Serialize)]
pub enum FileKind {
    Monodix,     // emits Stems, Paradigms
    Bidix,       // emits Stem
    MetaMonodix, // emits Stems, Paradigms
    MetaBidix,   // emits Stems
    Postdix,
    Rlx, // emits Rules
    Transfer, // emits Rules, Macros
    Lexc,
    Twol,
}

impl fmt::Display for FileKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl FileKind {
    #[allow(dead_code)]
    pub fn from_string(s: &str) -> Result<FileKind, String> {
        match s.to_lowercase().as_ref() {
            "monodix" => Ok(FileKind::Monodix),
            "bidix" => Ok(FileKind::Bidix),
            "metamonodix" => Ok(FileKind::MetaMonodix),
            "metabidix" => Ok(FileKind::MetaBidix),
            _ => Err(format!("Invalid file kind: {}", s)),
        }
    }
}

#[derive(Debug)]
pub enum StatsError {
    Hyper(hyper::Error),
    Utf8(str::Utf8Error),
    Io(io::Error),
    Xml(String),
    CgComp(String),
}

pub fn get_file_stats(
    file_path: &str,
    package_name: &str,
    file_kind: &FileKind,
) -> Result<Vec<(StatKind, String)>, StatsError> {
    let url = format!(
        "{}/{}/master/{}",
        super::ORGANIZATION_RAW_ROOT,
        package_name,
        file_path
    ).parse()
        .unwrap();

    let mut core = Core::new().unwrap(); // TODO: make these static/global?, move to utils.rs?
    let client = Client::configure()
        .connector(HttpsConnector::new(4, &core.handle()).unwrap())
        .build(&core.handle());

    let work = client
        .get(url)
        .map_err(StatsError::Hyper)
        .and_then(|response| {
            response
                .body()
                .concat2()
                .map_err(StatsError::Hyper)
                .and_then(move |body| match file_kind {
                    &FileKind::Monodix | &FileKind::MetaMonodix => {
                        let mut reader =
                            Reader::from_str(str::from_utf8(&*body).map_err(StatsError::Utf8)?);
                        let mut buf = Vec::new();

                        let mut e_count = 0;
                        let mut pardef_count = 0;
                        let mut in_section = false;
                        let mut in_pardefs = false;

                        loop {
                            match reader.read_event(&mut buf) {
                                Ok(Event::Start(ref e)) if e.name() == b"section" => {
                                    in_section = true
                                }
                                Ok(Event::Start(ref e)) if e.name() == b"pardefs" => {
                                    in_pardefs = true
                                }
                                Ok(Event::Start(ref e)) if in_section && e.name() == b"e" => {
                                    e_count += 1
                                }
                                Ok(Event::Start(ref e)) if in_pardefs && e.name() == b"pardef" => {
                                    pardef_count += 1
                                }
                                Ok(Event::End(ref e)) if e.name() == b"section" => {
                                    in_section = false
                                }
                                Ok(Event::End(ref e)) if e.name() == b"pardefs" => {
                                    in_pardefs = false
                                }
                                Ok(Event::Eof) => break,
                                Err(e) => {
                                    return Err(StatsError::Xml(format!(
                                        "Error at position {} in {}: {:?}",
                                        reader.buffer_position(),
                                        file_path,
                                        e
                                    )));
                                }
                                _ => (),
                            }
                            buf.clear();
                        }

                        Ok(vec![
                            (StatKind::Stems, e_count.to_string()),
                            (StatKind::Paradigms, pardef_count.to_string()),
                        ])
                    }
                    &FileKind::Bidix | &FileKind::MetaBidix => {
                        let mut reader =
                            Reader::from_str(str::from_utf8(&*body).map_err(StatsError::Utf8)?);
                        let mut buf = Vec::new();

                        let mut e_count = 0;
                        let mut in_section = false;

                        loop {
                            match reader.read_event(&mut buf) {
                                Ok(Event::Start(ref e)) if e.name() == b"section" => {
                                    in_section = true
                                }
                                Ok(Event::Start(ref e)) if in_section && e.name() == b"e" => {
                                    e_count += 1
                                }
                                Ok(Event::End(ref e)) if e.name() == b"section" => {
                                    in_section = false
                                }
                                Ok(Event::Eof) => break,
                                Err(e) => {
                                    return Err(StatsError::Xml(format!(
                                        "Error at position {} in {}: {:?}",
                                        reader.buffer_position(),
                                        file_path,
                                        e
                                    )));
                                }
                                _ => (),
                            }
                            buf.clear();
                        }

                        Ok(vec![(StatKind::Stems, e_count.to_string())])
                    }
                    &FileKind::Transfer => {
                        let mut reader =
                            Reader::from_str(str::from_utf8(&*body).map_err(StatsError::Utf8)?);
                        let mut buf = Vec::new();

                        let mut rule_count = 0;
                        let mut macro_count = 0;

                        loop {
                            match reader.read_event(&mut buf) {
                                Ok(Event::Start(ref e)) if e.name() == b"rule" => rule_count += 1,
                                Ok(Event::Start(ref e)) if e.name() == b"macro" => macro_count += 1,
                                Ok(Event::Eof) => break,
                                Err(e) => {
                                    return Err(StatsError::Xml(format!(
                                        "Error at position {} in {}: {:?}",
                                        reader.buffer_position(),
                                        file_path,
                                        e
                                    )));
                                }
                                _ => (),
                            }
                            buf.clear();
                        }

                        Ok(vec![
                            (StatKind::Rules, rule_count.to_string()),
                            (StatKind::Macros, macro_count.to_string()),
                        ])
                    }
                    &FileKind::Rlx => {
                        let mut rlx_file = NamedTempFile::new().map_err(StatsError::Io)?;
                        rlx_file.write_all(&*body).map_err(StatsError::Io)?;
                        let output = Command::new("cg-comp")
                            .arg(rlx_file.path().to_str().ok_or(StatsError::CgComp(
                                "Unable to create temporary file".to_string(),
                            ))?)
                            .arg("/dev/null")
                            .output();

                        match output {
                            Ok(Output {
                                status, ref stderr, ..
                            }) if status.success() =>
                            {
                                let cg_conv_output = String::from_utf8_lossy(&stderr);
                                lazy_static! {
                                    static ref RE: Regex = Regex::new(r"(\w+): (\d+)").unwrap();
                                }
                                for capture in RE.captures_iter(&cg_conv_output) {
                                    if &capture[1] == "Rules" {
                                        return Ok(vec![(StatKind::Rules, capture[2].to_string())]);
                                    }
                                }

                                Err(StatsError::CgComp(format!(
                                    "No stats in output: {}",
                                    &cg_conv_output
                                )))
                            }
                            Ok(Output { ref stderr, .. }) => Err(StatsError::CgComp(
                                String::from_utf8_lossy(&stderr).to_string(),
                            )),
                            Err(err) => Err(StatsError::Io(err)),
                        }
                    }
                    _ => Ok(vec![]), // TODO: rest of them
                })
        });

    core.run(work)
}

pub fn get_file_kind(file_name: &str) -> Option<FileKind> {
    lazy_static! {
        static ref RE: RegexSet = RegexSetBuilder::new(&[
            format!(r"^apertium-{re}\.{re}\.dix$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}-{re}\.{re}-{re}\.dix$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}\.{re}\.metadix$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}-{re}\.{re}\.metadix$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}-{re}\.{re}-{re}\.metadix$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}-{re}\.post-{re}\.dix$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}-{re}\.{re}-{re}\.rlx$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}\.{re}\.rlx$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}-{re}\.{re}-{re}\.t\dx$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}\.{re}\.lexc$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}-{re}\.{re}\.twol$", re=super::LANG_CODE_RE),
        ]).size_limit(50000000).build().unwrap();
    }

    let matches = RE.matches(file_name.trim_right_matches(".xml"));
    matches
        .into_iter()
        .collect::<Vec<_>>()
        .pop()
        .and_then(|i| match i {
            0 => Some(FileKind::Monodix),
            1 => Some(FileKind::Bidix),
            2 | 3 => Some(FileKind::MetaMonodix),
            4 => Some(FileKind::MetaBidix),
            5 => Some(FileKind::Postdix),
            6 | 7 => Some(FileKind::Rlx),
            8 => Some(FileKind::Transfer),
            9 => Some(FileKind::Lexc),
            10 => Some(FileKind::Twol),
            _ => None,
        })
}
