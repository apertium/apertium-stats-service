use std::{
    collections::{HashMap, HashSet},
    default::Default,
    error::Error,
    hash::BuildHasher,
    io::Write,
    ops::Try,
};

use diesel::{
    backend::Backend,
    deserialize::{self, FromSql},
    serialize::{self, Output, ToSql},
    sql_types::Binary,
    sqlite::Sqlite,
    types::IsNull,
};
use lazy_static::lazy_static;
use regex::Regex;
use rocket::{
    http::Status,
    response::{Responder, Response},
    FromForm, Request,
};
use rocket_contrib::json::JsonValue as RocketJsonValue;
use serde_derive::Serialize;

pub const LANG_CODE_RE: &str = r"(\w{2,3}?)(?:_(\w+))?";

lazy_static! {
    static ref ALPHA_CODE_MAP: &'static str = include_str!("../iso639.tsv");
    static ref ALPHA_1_TO_ALPHA_3: HashMap<&'static str, &'static str> = {
        ALPHA_CODE_MAP
            .lines()
            .map(|l| {
                let split = l.split('\t').collect::<Vec<_>>();
                (split[0], split[1])
            })
            .collect()
    };
    static ref ALPHA_3_TO_ALPHA_1: HashMap<&'static str, &'static str> = {
        ALPHA_CODE_MAP
            .lines()
            .map(|l| {
                let split = l.split('\t').collect::<Vec<_>>();
                (split[1], split[0])
            })
            .collect()
    };
}

fn convert_language_code(code: &str, sub_code: Option<&str>) -> Option<String> {
    let converted_code = match code.len() {
        3 => ALPHA_3_TO_ALPHA_1.get(code),
        2 => ALPHA_1_TO_ALPHA_3.get(code),
        _ => None,
    };

    sub_code.map_or_else(
        || converted_code.map(|x| x.to_string()),
        |y| converted_code.map(|x| format!("{}_{}", x, y)),
    )
}

pub fn normalize_name<H: BuildHasher>(name: &str, package_names: HashSet<String, H>) -> Result<String, String> {
    let normalized_name = if name.starts_with("apertium-") {
        name.to_string()
    } else {
        format!("apertium-{}", name)
    };

    lazy_static! {
        static ref MODULE_RE: Regex = Regex::new(&format!(r"^apertium-{re}$", re = LANG_CODE_RE)).unwrap();
        static ref PAIR_RE: Regex = Regex::new(&format!(r"^apertium-{re}-{re}$", re = LANG_CODE_RE)).unwrap();
    }

    if package_names.contains(&normalized_name) {
        return Ok(normalized_name);
    }

    let mut format_matches = false;

    if let Some(converted_name) = {
        if let Some((Some(language_code), language_sub_code)) = MODULE_RE
            .captures(&normalized_name)
            .map(|x| (x.get(1).map(|x| x.as_str()), x.get(2).map(|x| x.as_str())))
        {
            format_matches = true;
            convert_language_code(language_code, language_sub_code).map(|x| format!("apertium-{}", x))
        } else if let Some(captures) = PAIR_RE.captures(&normalized_name) {
            let (language_code_1, language_code_2) = (&captures[1], &captures[3]);
            let (language_sub_code_1, language_sub_code_2) =
                (captures.get(2).map(|x| x.as_str()), captures.get(4).map(|x| x.as_str()));
            format_matches = true;
            if let (Some(converted_language_code_1), Some(converted_language_code_2)) = (
                convert_language_code(language_code_1, language_sub_code_1),
                convert_language_code(language_code_2, language_sub_code_2),
            ) {
                Some(format!(
                    "apertium-{}-{}",
                    converted_language_code_1, converted_language_code_2
                ))
            } else {
                None
            }
        } else {
            None
        }
    } {
        if package_names.contains(&converted_name) {
            return Ok(converted_name);
        }
    }

    if format_matches {
        Ok(normalized_name)
    } else {
        Err(format!("Invalid package name: {}", name))
    }
}

pub enum JsonResult {
    Ok(RocketJsonValue),
    Err(Option<RocketJsonValue>, Status),
}

impl Try for JsonResult {
    type Ok = RocketJsonValue;
    type Error = (Option<RocketJsonValue>, Status);

    fn into_result(self) -> Result<<Self as Try>::Ok, Self::Error> {
        match self {
            JsonResult::Ok(value) => Ok(value),
            JsonResult::Err(value, status) => Err((value, status)),
        }
    }

    fn from_error((value, status): Self::Error) -> Self {
        JsonResult::Err(value, status)
    }

    fn from_ok(value: <Self as Try>::Ok) -> Self {
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
pub struct JsonValue(pub RocketJsonValue);

impl FromSql<JsonType, Sqlite> for JsonValue {
    fn from_sql(value: Option<&<Sqlite as Backend>::RawValue>) -> deserialize::Result<Self> {
        let bytes = <*const [u8] as FromSql<Binary, Sqlite>>::from_sql(not_none!(value.into()))?;
        serde_json::from_slice(unsafe { &*bytes })
            .map(JsonValue)
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }
}

impl ToSql<JsonType, Sqlite> for JsonValue {
    fn to_sql<W: Write>(&self, out: &mut Output<W, Sqlite>) -> serialize::Result {
        serde_json::to_writer(out, &self.0)
            .map(|_| if self.0.is_null() { IsNull::Yes } else { IsNull::No })
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }
}

impl From<RocketJsonValue> for JsonValue {
    fn from(value: RocketJsonValue) -> Self {
        JsonValue(value)
    }
}

#[derive(FromForm)]
pub struct Params {
    pub recursive: Option<bool>,

    #[form(field = "async")]
    pub r#async: Option<bool>,
}

impl Params {
    pub fn is_async(&self) -> bool {
        self.r#async.unwrap_or(true)
    }

    pub fn is_recursive(&self) -> bool {
        self.recursive.unwrap_or(false)
    }
}

impl Default for Params {
    fn default() -> Self {
        Self {
            recursive: None,
            r#async: Some(true),
        }
    }
}
