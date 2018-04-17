extern crate hyper;
extern crate quick_xml;

use std::str;

use self::quick_xml::events::Event;
use self::quick_xml::reader::Reader;

use super::StatsError;

use models::StatKind;

pub fn get_bidix_stats(
    body: hyper::Chunk,
    file_path: &str,
) -> Result<Vec<(StatKind, String)>, StatsError> {
    let mut reader = Reader::from_str(str::from_utf8(&*body).map_err(StatsError::Utf8)?);
    let mut buf = Vec::new();

    let mut e_count = 0;
    let mut in_section = false;

    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) if e.name() == b"section" => in_section = true,
            Ok(Event::Start(ref e)) if in_section && e.name() == b"e" => e_count += 1,
            Ok(Event::End(ref e)) if e.name() == b"section" => in_section = false,
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

pub fn get_monodix_stats(
    body: hyper::Chunk,
    file_path: &str,
) -> Result<Vec<(StatKind, String)>, StatsError> {
    let mut reader = Reader::from_str(str::from_utf8(&*body).map_err(StatsError::Utf8)?);
    let mut buf = Vec::new();

    let mut e_count = 0;
    let mut pardef_count = 0;
    let mut in_section = false;
    let mut in_pardefs = false;

    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) if e.name() == b"section" => in_section = true,
            Ok(Event::Start(ref e)) if e.name() == b"pardefs" => in_pardefs = true,
            Ok(Event::Start(ref e)) if in_section && e.name() == b"e" => e_count += 1,
            Ok(Event::Start(ref e)) if in_pardefs && e.name() == b"pardef" => pardef_count += 1,
            Ok(Event::End(ref e)) if e.name() == b"section" => in_section = false,
            Ok(Event::End(ref e)) if e.name() == b"pardefs" => in_pardefs = false,
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

pub fn get_transfer_stats(
    body: hyper::Chunk,
    file_path: &str,
) -> Result<Vec<(StatKind, String)>, StatsError> {
    let mut reader = Reader::from_str(str::from_utf8(&*body).map_err(StatsError::Utf8)?);
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
