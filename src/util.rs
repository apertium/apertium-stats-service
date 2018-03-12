use regex::RegexSet;
use rocket_contrib::{Json, Value};
use rocket::http::Status;
use rocket::response::{Responder, Response};
use rocket::Request;

pub fn normalize_name(name: &str) -> String {
    if name.starts_with("apertium-") {
        name.to_string()
    } else {
        format!("apertium-{}", name)
    }
}

pub fn is_valid_name(name: &str) -> bool {
    lazy_static! {
        static ref RE: RegexSet = RegexSet::new(&[
            format!(r"^apertium-({re})$", re=super::LANG_CODE_RE),
            format!(r"^apertium-({re})-({re})$", re=super::LANG_CODE_RE),
        ]).unwrap();
    }

    RE.matches(name).matched_any()
}

pub enum JsonResult {
    Err(Option<Json<Value>>, Status),
    Ok(Json<Value>),
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
