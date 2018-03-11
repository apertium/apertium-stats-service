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
    Monodix,
    Bidix,
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
        response.body().concat2().and_then(move |body| {
            match file_kind {
                &FileKind::Monodix => {
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
                                panic!("Error at position {}: {:?}", reader.buffer_position(), e)
                            }
                            _ => (),
                        }
                        buf.clear();
                    }

                    Ok(vec![
                        (StatKind::Stems, e_count.to_string()),
                        (StatKind::Paradigms, pardef_count.to_string()),
                    ])
                },
                &FileKind::Bidix => {
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
                                panic!("Error at position {}: {:?}", reader.buffer_position(), e)
                            }
                            _ => (),
                        }
                        buf.clear();
                    }

                    Ok(vec![
                        (StatKind::Stems, e_count.to_string()),
                    ])
                },
            }
        })
    });

    core.run(work)
}

pub fn get_file_kind(file_name: &str) -> Option<FileKind> {
    lazy_static! {
        static ref RE: RegexSet = RegexSet::new(&[
            format!(r"^apertium-({re})\.({re})\.dix$", re=super::LANG_CODE_RE),
            format!(r"^apertium-({re})-({re})\.({re})-({re})\.dix$", re=super::LANG_CODE_RE),
        ]).unwrap();
    }

    let matches = RE.matches(file_name);
    if matches.matched(0) {
        Some(FileKind::Monodix)
    } else if matches.matched(1) {
        Some(FileKind::Bidix)
    } else {
        // TODO: implement the rest
        None
    }
}
