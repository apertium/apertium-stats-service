extern crate serde_json;

use std::str;

use chrono::NaiveDateTime;

use super::schema::entries;

#[derive(PartialEq, Clone, Debug, Serialize, DbEnum)]
pub enum FileKind {
    Monodix,     // emits Stems, Paradigms
    Bidix,       // emits Stem
    MetaMonodix, // emits Stems, Paradigms
    MetaBidix,   // emits Stems
    Postdix,
    Rlx,      // emits Rules
    Transfer, // emits Rules, Macros
    Lexc,
    Twol,
}

impl FileKind {
    pub fn from_string(s: &str) -> Result<FileKind, String> {
        match s.to_lowercase().replace("_", "").as_ref() {
            "monodix" => Ok(FileKind::Monodix),
            "bidix" => Ok(FileKind::Bidix),
            "metamonodix" => Ok(FileKind::MetaMonodix),
            "metabidix" => Ok(FileKind::MetaBidix),
            _ => Err(format!("Invalid file kind: {}", s)),
        }
    }
}


#[derive(PartialEq, Clone, Debug, Serialize, DbEnum)]
pub enum StatKind {
    Stems,
    Paradigms,
    Rules,
    Macros,
}

#[derive(Queryable, Serialize)]
pub struct Entry {
    #[serde(skip_serializing)]
    pub id: i32,

    pub requested: NaiveDateTime,
    pub created: NaiveDateTime,
    pub name: String,
    pub revision: i32,
    pub path: String,
    pub file_kind: FileKind,
    pub stat_kind: StatKind,
    pub value: String,
}

#[derive(Queryable, Insertable, Debug)]
#[table_name = "entries"]
pub struct NewEntry<'a> {
    pub requested: &'a NaiveDateTime,
    pub created: NaiveDateTime,
    pub name: &'a str,
    pub revision: &'a i32,
    pub path: &'a str,
    pub file_kind: FileKind,
    pub stat_kind: StatKind,
    pub value: String,
}
