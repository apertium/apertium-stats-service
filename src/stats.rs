extern crate futures;
extern crate hyper;
extern crate hyper_tls;
extern crate quick_xml;
extern crate tempfile;
extern crate tokio_core;

use regex::{Regex, RegexSet, RegexSetBuilder};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::collections::hash_map::Entry;
use std::io::{self, BufRead, BufReader, Write};
use std::iter::FromIterator;
use std::process::{Command, Output};
use std::str;

use self::futures::{Future, Stream};
use self::hyper::Client;
use self::tokio_core::reactor::Core;
use self::hyper_tls::HttpsConnector;
use self::tempfile::NamedTempFile;
use self::quick_xml::reader::Reader;
use self::quick_xml::events::Event;

use super::models::{FileKind, StatKind};

#[derive(Debug)]
pub enum StatsError {
    Hyper(hyper::Error),
    Utf8(str::Utf8Error),
    Io(io::Error),
    Xml(String),
    CgComp(String),
    Lexc(String),
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

    let mut core = Core::new().unwrap();
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
                .and_then(move |body| match *file_kind {
                    FileKind::Monodix | FileKind::MetaMonodix => {
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
                            (StatKind::Entries, e_count.to_string()),
                            (StatKind::Paradigms, pardef_count.to_string()),
                        ])
                    }
                    FileKind::Bidix | FileKind::MetaBidix | FileKind::Postdix => {
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

                        Ok(vec![(StatKind::Entries, e_count.to_string())])
                    }
                    FileKind::Transfer => {
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
                    FileKind::Rlx => {
                        let mut rlx_file = NamedTempFile::new().map_err(StatsError::Io)?;
                        rlx_file.write_all(&*body).map_err(StatsError::Io)?;
                        let output = Command::new("cg-comp")
                            .arg(rlx_file.path().to_str().ok_or_else(|| {
                                StatsError::CgComp("Unable to create temporary file".to_string())
                            })?)
                            .arg("/dev/null")
                            .output();

                        match output {
                            Ok(Output {
                                status, ref stderr, ..
                            }) if status.success() =>
                            {
                                let cg_conv_output = String::from_utf8_lossy(stderr);
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
                                String::from_utf8_lossy(stderr).to_string(),
                            )),
                            Err(err) => Err(StatsError::Io(err)),
                        }
                    }
                    FileKind::Twol => {
                        let rule_count = BufReader::new(&*body)
                            .lines()
                            .filter(|line_result| {
                                line_result
                                    .as_ref()
                                    .ok()
                                    .map_or(false, |line| line.starts_with('"'))
                            })
                            .count();
                        Ok(vec![(StatKind::Rules, rule_count.to_string())])
                    }
                    FileKind::Lexc => {
                        let mut current_lexicon: Option<String> = None;
                        let mut lexicons: HashMap<String, (Vec<String>, HashSet<(String, BTreeSet<String>)>)> = HashMap::new();

                        lazy_static! {
                            static ref CLEAN_RE: Regex = Regex::new(r"%(.)").unwrap(); // TODO: better name
                            static ref CLEAN_COMMENTS_RE: Regex = Regex::new(r"!.*$").unwrap();
                            // static ref
                        }

                        for maybe_line in BufReader::new(&*body).lines() {
                            if let Ok(line) = maybe_line {
                                let clean_line_intermediate = CLEAN_RE.replace(&line, r"\1");
                                let clean_line = CLEAN_COMMENTS_RE.replace(&clean_line_intermediate, "");
                                if clean_line.starts_with("LEXICON") {
                                    let lexicon_name = clean_line
                                        .split_whitespace().nth(1)
                                        .ok_or_else(|| StatsError::Lexc(format!("LEXICON start missing <space> (L{})", 1)))?; // TODO: real line
                                    current_lexicon = Some(lexicon_name.to_string());
                                } else if !clean_line.is_empty() && current_lexicon.is_some() {
                                    let line_error = format!("Unable to parse L{}", 1); // TODO: real line
                                    let token_count = clean_line.split_whitespace().count();

                                    if token_count >= 2 {
                                        if clean_line.contains(':') {
                                            // TODO: me
                                        } else {
                                            let mut split = clean_line
                                                .split(';')
                                                .next()
                                                .ok_or_else(|| StatsError::Lexc(line_error.clone()))?
                                                .trim()
                                                .split_whitespace();
                                            let lemma = split
                                                .next()
                                                .ok_or_else(|| StatsError::Lexc(line_error.clone()))?;
                                            let continuation_lexicon = split
                                                .next()
                                                .ok_or_else(|| StatsError::Lexc(line_error.clone()))?
                                                .trim()
                                                .split('-')
                                                .map(|x| x.to_string())
                                                .collect::<BTreeSet<_>>();
                                            // let gloss = if clean_line.contains('!') {
                                            //     Some(clean_line.split('!').nth(1))
                                            // } else {
                                            //     None
                                            // };

                                            match lexicons.entry(current_lexicon.as_ref().unwrap().to_string()) {
                                                Entry::Occupied(mut occupied) => {
                                                    occupied.get_mut().1.insert((lemma.to_string(), continuation_lexicon));
                                                }
                                                Entry::Vacant(vacant) => {
                                                    vacant.insert(
                                                        (vec![], HashSet::from_iter(vec![(lemma.to_string(), continuation_lexicon)]))
                                                    );
                                                }
                                            };
                                        }
                                    } else if token_count == 1 {
                                        let lexicon_pointer = clean_line
                                            .split(';')
                                            .next()
                                            .ok_or_else(|| StatsError::Lexc(line_error.clone()))?
                                            .trim();
                                        if lexicon_pointer.contains(' ') {
                                            // TODO: log error
                                        } else {
                                            match lexicons.entry(current_lexicon.as_ref().unwrap().to_string()) {
                                                Entry::Occupied(mut occupied) => {
                                                    occupied.get_mut().0.push(lexicon_pointer.to_string());
                                                }
                                                Entry::Vacant(vacant) => {
                                                    vacant.insert((vec![lexicon_pointer.to_string()], HashSet::new()));
                                                }
                                            };
                                        }
                                    } else {
                                        // TODO: log parse failure
                                    }
                                }
                            }
                        }

                        Ok(vec![])
                    }
                    // _ => Ok(vec![]),
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
            format!(r"^apertium-{re}\.{re}\.twol$", re=super::LANG_CODE_RE),
        ]).size_limit(50_000_000).build().unwrap();
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
            10 | 11 => Some(FileKind::Twol),
            _ => None,
        })
}
