use std::{fmt, str};

use chrono::NaiveDateTime;
use diesel_derive_enum::DbEnum;
use serde_derive::Serialize;

use crate::{schema::entries, util::JsonValue};

#[derive(PartialEq, Clone, Debug, Serialize, DbEnum)]
pub enum FileKind {
    Monodix,     // emits Stems, Paradigms
    Bidix,       // emits Entries
    MetaMonodix, // emits Entries, Paradigms
    MetaBidix,   // emits Entries
    Postdix,     // emits Entries
    Rlx,         // emits Rules
    Transfer,    // emits Rules, Macros
    Lexc,        // emits Stems, VanillaStems
    Twol,        // emits Rules
    Lexd,        // emits Lexicons, LexiconEntries, Patterns, PatternEntries
}

impl FileKind {
    pub fn from_string(s: &str) -> Result<FileKind, String> {
        match s.to_lowercase().replace("_", "").as_ref() {
            "monodix" => Ok(FileKind::Monodix),
            "bidix" => Ok(FileKind::Bidix),
            "metamonodix" => Ok(FileKind::MetaMonodix),
            "metabidix" => Ok(FileKind::MetaBidix),
            "postdix" => Ok(FileKind::Postdix),
            "rlx" => Ok(FileKind::Rlx),
            "transfer" => Ok(FileKind::Transfer),
            "lexc" => Ok(FileKind::Lexc),
            "twol" => Ok(FileKind::Twol),
            "lexd" => Ok(FileKind::Lexd),
            _ => Err(format!("Invalid file kind: {}", s)),
        }
    }
}

impl fmt::Display for FileKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(PartialEq, Clone, Debug, Serialize, DbEnum)]
pub enum StatKind {
    Entries,
    Paradigms,
    Rules,
    Macros,
    Stems,
    VanillaStems,
    Lexicons,
    LexiconEntries,
    Patterns,
    PatternEntries,
}

#[derive(QueryableByName, Queryable, Serialize)]
#[table_name = "entries"]
pub struct Entry {
    #[serde(skip_serializing)]
    pub id: i32,

    pub requested: NaiveDateTime,
    pub created: NaiveDateTime,
    pub name: String,
    pub revision: i32,
    pub sha: String,
    pub path: String,
    pub last_changed: NaiveDateTime,
    pub last_author: String,
    pub size: i32,
    pub file_kind: FileKind,
    pub stat_kind: StatKind,
    pub value: JsonValue,
}

#[derive(Clone, Insertable, Debug, Serialize)]
#[table_name = "entries"]
pub struct NewEntry {
    pub requested: NaiveDateTime,
    pub created: NaiveDateTime,
    pub name: String,
    pub revision: i32,
    pub sha: String,
    pub path: String,
    pub last_changed: NaiveDateTime,
    pub last_author: String,
    pub size: i32,
    pub file_kind: FileKind,
    pub stat_kind: StatKind,
    pub value: JsonValue,
}
