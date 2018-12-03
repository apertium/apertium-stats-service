use std::{default::Default, error::Error, io::Write, ops::Try};

use diesel::{
    backend::Backend,
    deserialize::{self, FromSql},
    serialize::{self, Output, ToSql},
    sql_types::Binary,
    sqlite::Sqlite,
    types::IsNull,
};
use regex::RegexSet;
use rocket::{
    http::Status,
    response::{Responder, Response},
    Request,
};
use rocket_contrib::{Json, Value};
use serde_json;

use LANG_CODE_RE;

pub fn normalize_name(name: &str) -> Result<String, String> {
    let normalized_name = if name.starts_with("apertium-") {
        name.to_string()
    } else {
        format!("apertium-{}", name)
    };

    lazy_static! {
        static ref RE: RegexSet = RegexSet::new(&[
            format!(r"^apertium-({re})$", re = LANG_CODE_RE),
            format!(r"^apertium-({re})-({re})$", re = LANG_CODE_RE),
        ])
        .unwrap();
    }

    if RE.matches(&normalized_name).matched_any() {
        Ok(normalized_name)
    } else {
        Err(format!("Invalid package name: {}", name))
    }
}

pub enum JsonResult {
    Ok(Json<Value>),
    Err(Option<Json<Value>>, Status),
}

impl Try for JsonResult {
    type Ok = Json<Value>;
    type Error = (Option<Json<Value>>, Status);

    fn into_result(self) -> Result<Self::Ok, Self::Error> {
        match self {
            JsonResult::Ok(value) => Ok(value),
            JsonResult::Err(value, status) => Err((value, status)),
        }
    }

    fn from_error((value, status): Self::Error) -> Self {
        JsonResult::Err(value, status)
    }

    fn from_ok(value: Self::Ok) -> Self {
        JsonResult::Ok(value)
    }
}

impl<'r> Responder<'r> for JsonResult {
    fn respond_to(self, req: &Request) -> Result<Response<'r>, Status> {
        match self {
            JsonResult::Ok(value) => value.respond_to(req),
            JsonResult::Err(maybe_value, status) => match maybe_value {
                Some(value) => match value.respond_to(req) {
                    Ok(mut response) => {
                        response.set_status(status);
                        Ok(response)
                    },
                    err => err,
                },
                None => Err(status),
            },
        }
    }
}

#[derive(SqlType)]
#[sqlite_type = "Text"]
pub struct JsonType;

#[derive(AsExpression, Debug, Clone, Serialize, FromSqlRow)]
#[sql_type = "JsonType"]
pub struct JsonValue(pub Value);

impl FromSql<JsonType, Sqlite> for JsonValue {
    fn from_sql(value: Option<&<Sqlite as Backend>::RawValue>) -> deserialize::Result<Self> {
        let bytes = <*const [u8] as FromSql<Binary, Sqlite>>::from_sql(not_none!(value.into()))?;
        serde_json::from_slice(unsafe { &*bytes })
            .map(JsonValue)
            .map_err(|e| Box::new(e) as Box<Error + Send + Sync>)
    }
}

impl ToSql<JsonType, Sqlite> for JsonValue {
    fn to_sql<W: Write>(&self, out: &mut Output<W, Sqlite>) -> serialize::Result {
        serde_json::to_writer(out, &self.0)
            .map(|_| if self.0.is_null() { IsNull::Yes } else { IsNull::No })
            .map_err(|e| Box::new(e) as Box<Error + Send + Sync>)
    }
}

impl From<Value> for JsonValue {
    fn from(value: Value) -> Self {
        JsonValue(value)
    }
}

#[derive(FromForm)]
pub struct Params {
    pub recursive: Option<bool>,
    pub async: Option<bool>,
}

impl Params {
    pub fn is_async(&self) -> bool {
        self.async.unwrap_or(true)
    }

    pub fn is_recursive(&self) -> bool {
        self.recursive.unwrap_or(false)
    }
}

impl Default for Params {
    fn default() -> Self {
        Self {
            recursive: None,
            async: Some(true),
        }
    }
}
