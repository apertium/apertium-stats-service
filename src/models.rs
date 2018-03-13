extern crate serde_json;

use chrono::NaiveDateTime;

use super::schema::entries;

#[derive(Queryable, Serialize)]
pub struct Entry {
    #[serde(skip_serializing)]
    pub id: i32,

    pub requested: NaiveDateTime,
    pub created: NaiveDateTime,
    pub name: String,
    pub revision: i32,
    pub path: String,
    pub file_kind: String,
    pub stat_kind: String,
    pub value: String,
}

#[derive(Insertable)]
#[table_name = "entries"]
pub struct NewEntry<'a> {
    pub requested: &'a NaiveDateTime,
    pub created: NaiveDateTime,
    pub name: &'a str,
    pub revision: &'a i32,
    pub path: &'a str,
    pub file_kind: String,
    pub stat_kind: String,
    pub value: String,
}
