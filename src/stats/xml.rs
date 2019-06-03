use std::str;

use hyper::Chunk;
use quick_xml::{
    events::{attributes::Attribute, Event},
    Reader,
};
use rocket_contrib::json::JsonValue;

use models::StatKind;
use stats::StatsError;

pub fn get_bidix_stats(body: Chunk, file_path: &str) -> Result<Vec<(StatKind, JsonValue)>, StatsError> {
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
            },
            _ => (),
        }
        buf.clear();
    }

    Ok(vec![(StatKind::Entries, json!(e_count))])
}

pub fn get_monodix_stats(body: Chunk, file_path: &str) -> Result<Vec<(StatKind, JsonValue)>, StatsError> {
    let mut reader = Reader::from_str(str::from_utf8(&*body).map_err(StatsError::Utf8)?);
    let mut buf = Vec::new();

    let mut stem_count = 0;
    let mut pardef_count = 0;
    let mut in_section = false;
    let mut in_pardefs = false;

    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) if e.name() == b"section" => in_section = true,
            Ok(Event::Start(ref e)) if e.name() == b"pardefs" => in_pardefs = true,
            Ok(Event::Start(ref e)) if in_section && e.name() == b"e" => {
                if e.attributes()
                    .any(|a| a.ok().map_or(false, |Attribute { key, .. }| key == b"lm"))
                {
                    stem_count += 1
                }
            },
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
            },
            _ => (),
        }
        buf.clear();
    }

    Ok(vec![
        (StatKind::Stems, json!(stem_count)),
        (StatKind::Paradigms, json!(pardef_count)),
    ])
}

pub fn get_transfer_stats(body: Chunk, file_path: &str) -> Result<Vec<(StatKind, JsonValue)>, StatsError> {
    let mut reader = Reader::from_str(str::from_utf8(&*body).map_err(StatsError::Utf8)?);
    let mut buf = Vec::new();

    let mut rule_count = 0;
    let mut macro_count = 0;

    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) if e.name() == b"rule" => rule_count += 1,
            Ok(Event::Start(ref e)) if e.name() == b"def-macro" => macro_count += 1,
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(StatsError::Xml(format!(
                    "Error at position {} in {}: {:?}",
                    reader.buffer_position(),
                    file_path,
                    e
                )));
            },
            _ => (),
        }
        buf.clear();
    }

    Ok(vec![
        (StatKind::Rules, json!(rule_count)),
        (StatKind::Macros, json!(macro_count)),
    ])
}
