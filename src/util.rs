use std::ops::Try;
use std::default::Default;

use regex::RegexSet;
use rocket_contrib::{Json, Value};
use rocket::http::Status;
use rocket::response::{Responder, Response};
use rocket::Request;

pub fn normalize_name(name: &str) -> Result<String, String> {
    let normalized_name = if name.starts_with("apertium-") {
        name.to_string()
    } else {
        format!("apertium-{}", name)
    };

    lazy_static! {
        static ref RE: RegexSet = RegexSet::new(&[
            format!(r"^apertium-({re})$", re=super::LANG_CODE_RE),
            format!(r"^apertium-({re})-({re})$", re=super::LANG_CODE_RE),
        ]).unwrap();
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

    fn from_ok(value: Self::Ok) -> Self {
        JsonResult::Ok(value)
    }

    fn from_error((value, status): Self::Error) -> Self {
        JsonResult::Err(value, status)
    }

    fn into_result(self) -> Result<Self::Ok, Self::Error> {
        match self {
            JsonResult::Ok(value) => Ok(value),
            JsonResult::Err(value, status) => Err((value, status)),
        }
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
                    }
                    err => err,
                },
                None => Err(status),
            },
        }
    }
}

#[derive(FromForm)]
pub struct Params {
    pub recursive: Option<bool>,
    pub wait: Option<bool>,
}

impl Params {
    pub fn is_recursive(&self) -> bool {
        self.recursive.unwrap_or(false)
    }
}

impl Default for Params {
    fn default() -> Params {
        Params {
            recursive: None,
            wait: None,
        }
    }
}
