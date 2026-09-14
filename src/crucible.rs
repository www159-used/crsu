use crate::review_diff::ReviewDiff;
use serde_json::{Value, json};
use std::env;
use std::fmt;

pub fn submit_if_configured(review_diff: &ReviewDiff) -> Result<Option<String>, CrucibleError> {
    let Some(config) = Config::from_environment()? else {
        return Ok(None);
    };

    let response = reqwest::blocking::Client::new()
        .post(format!("{}/rest-service/reviews-v1", config.url))
        .query(&[("FEAUTH", config.token)])
        .json(&json!({
            "reviewData": {
                "projectKey": config.project,
                "name": review_diff.title(),
            },
            "patch": review_diff.patch(),
        }))
        .send()
        .map_err(CrucibleError::Request)?
        .error_for_status()
        .map_err(CrucibleError::Request)?
        .json::<Value>()
        .map_err(CrucibleError::Request)?;

    let review_id = response
        .pointer("/permaId/id")
        .and_then(Value::as_str)
        .ok_or(CrucibleError::MalformedResponse)?;
    Ok(Some(review_id.to_owned()))
}

struct Config {
    url: String,
    project: String,
    token: String,
}

impl Config {
    fn from_environment() -> Result<Option<Self>, CrucibleError> {
        let url = env::var("CRSU_CRUCIBLE_URL").ok();
        let project = env::var("CRSU_CRUCIBLE_PROJECT").ok();
        let token = env::var("CRSU_CRUCIBLE_TOKEN").ok();
        if url.is_none() && project.is_none() && token.is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            url: required("CRSU_CRUCIBLE_URL", url)?,
            project: required("CRSU_CRUCIBLE_PROJECT", project)?,
            token: required("CRSU_CRUCIBLE_TOKEN", token)?,
        }))
    }
}

fn required(name: &'static str, value: Option<String>) -> Result<String, CrucibleError> {
    value.ok_or(CrucibleError::MissingConfiguration(name))
}

#[derive(Debug)]
pub enum CrucibleError {
    MalformedResponse,
    MissingConfiguration(&'static str),
    Request(reqwest::Error),
}

impl fmt::Display for CrucibleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedResponse => write!(formatter, "Crucible response has no review id"),
            Self::MissingConfiguration(name) => write!(formatter, "missing {name}"),
            Self::Request(error) => write!(formatter, "Crucible request failed: {error}"),
        }
    }
}
