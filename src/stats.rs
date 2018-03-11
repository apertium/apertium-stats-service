extern crate futures;
extern crate hyper;
extern crate hyper_tls;
extern crate quick_xml;
extern crate tokio_core;

use regex::RegexSet;
use std::fmt;
use std::str;

use self::futures::{Future, Stream};
use self::hyper::Client;
use self::tokio_core::reactor::Core;
use self::hyper_tls::HttpsConnector;
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
    Monodix, // emits Stems, Paradigms
    Bidix, // emits Stem
    MetaMonodix, // emits Stems, Paradigms
    MetaBidix, // emits Stems
    Postdix,
    Rlx,
    Transfer,
    Lexc,
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

pub fn get_file_stats(
    file_path: &str,
    package_name: &str,
    file_kind: &FileKind,
) -> Result<Vec<(StatKind, String)>, hyper::Error> {
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

    let work = client.get(url).and_then(|response| {
        response
            .body()
            .concat2()
            .and_then(move |body| match file_kind {
                &FileKind::Monodix | &FileKind::MetaMonodix => {
                    let mut reader = Reader::from_str(&str::from_utf8(&*body).unwrap());
                    let mut buf = Vec::new();

                    let mut e_count = 0;
                    let mut pardef_count = 0;
                    let mut in_section = false;
                    let mut in_pardefs = false;

                    loop {
                        match reader.read_event(&mut buf) {
                            Ok(Event::Start(ref e)) if e.name() == b"section" => in_section = true,
                            Ok(Event::Start(ref e)) if e.name() == b"pardefs" => in_pardefs = true,
                            Ok(Event::Start(ref e)) if in_section && e.name() == b"e" => {
                                e_count += 1
                            }
                            Ok(Event::Start(ref e)) if in_pardefs && e.name() == b"pardef" => {
                                pardef_count += 1
                            }
                            Ok(Event::End(ref e)) if e.name() == b"section" => in_section = false,
                            Ok(Event::End(ref e)) if e.name() == b"pardefs" => in_pardefs = false,
                            Ok(Event::Eof) => break,
                            Err(e) => {
                                println!(
                                    "Error at position {} in {}: {:?}",
                                    reader.buffer_position(),
                                    file_path,
                                    e
                                ); // TODO: log instead
                                return Ok(vec![]); // TODO: pass up Err instead
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
                    let mut reader = Reader::from_str(&str::from_utf8(&*body).unwrap());
                    let mut buf = Vec::new();

                    let mut e_count = 0;
                    let mut in_section = false;

                    loop {
                        match reader.read_event(&mut buf) {
                            Ok(Event::Start(ref e)) if e.name() == b"section" => in_section = true,
                            Ok(Event::Start(ref e)) if in_section && e.name() == b"e" => {
                                e_count += 1
                            }
                            Ok(Event::End(ref e)) if e.name() == b"section" => in_section = false,
                            Ok(Event::Eof) => break,
                            Err(e) => {
                                println!(
                                    "Error at position {} in {}: {:?}",
                                    reader.buffer_position(),
                                    file_path,
                                    e
                                ); // TODO: log instead
                                return Ok(vec![]); // TODO: pass up Err instead
                            }
                            _ => (),
                        }
                        buf.clear();
                    }

                    Ok(vec![(StatKind::Stems, e_count.to_string())])
                }
                _ => Ok(vec![])
            })
    });

    core.run(work)
}

pub fn get_file_kind(file_name: &str) -> Option<FileKind> {
    lazy_static! {
        static ref RE: RegexSet = RegexSet::new(&[
            format!(r"^apertium-{re}\.{re}\.dix$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}-{re}\.{re}-{re}\.dix$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}\.{re}\.metadix$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}-{re}\.{re}\.metadix$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}-{re}\.{re}-{re}\.metadix$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}-{re}\.post-{re}\.dix$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}-{re}\.{re}-{re}\.rlx$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}-{re}\.{re}-{re}\.t\dx$", re=super::LANG_CODE_RE),
            format!(r"^apertium-{re}\.{re}\.lexc$", re=super::LANG_CODE_RE),
        ]).unwrap();
    }

    let matches = RE.matches(file_name.trim_right_matches(".xml"));
    matches.into_iter().collect::<Vec<_>>().pop().and_then(|i| match i {
        0 => Some(FileKind::Monodix),
        1 => Some(FileKind::Bidix),
        2 | 3 => Some(FileKind::MetaMonodix),
        4 => Some(FileKind::MetaBidix),
        5 => Some(FileKind::Postdix),
        6 => Some(FileKind::Rlx),
        7 => Some(FileKind::Transfer),
        8 => Some(FileKind::Lexc),
        _ => None
    })
}
