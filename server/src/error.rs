//! API error type: every handler returns `Result<_, ApiError>`, rendered as
//! `{"error": "<message>"}` with the right HTTP status.

use crate::i18n::Text;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    /// Rendered in the language of the request (see `i18n`).
    pub message: Text,
    /// Extra fields merged into the JSON body (e.g. the certificate to review).
    pub details: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Where the cause of an error waits in `details`, taken out before the body is sent.
const CAUSE: &str = "\u{0}cause";

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<Text>) -> Self {
        Self { status, message: message.into(), details: None }
    }
    pub fn with_detail(mut self, key: &str, value: impl serde::Serialize) -> Self {
        let value = serde_json::to_value(value).unwrap_or_default();
        self.details.get_or_insert_with(Default::default).insert(key.to_string(), value);
        self
    }
    /// What went wrong inside, for the administrators' list of errors (never sent to the client).
    pub fn with_cause(mut self, cause: impl Into<String>) -> Self {
        self.details.get_or_insert_with(Default::default).insert(CAUSE.into(), serde_json::Value::String(cause.into()));
        self
    }
    pub fn bad_request(message: impl Into<Text>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
    pub fn unauthorized(message: impl Into<Text>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }
    pub fn forbidden(message: impl Into<Text>) -> Self {
        Self::new(StatusCode::FORBIDDEN, message)
    }
    pub fn not_found(message: impl Into<Text>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }
    pub fn conflict(message: impl Into<Text>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }
    pub fn bad_gateway(message: impl Into<Text>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, message)
    }
    pub fn internal(message: impl Into<Text>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = self.details.unwrap_or_default();
        let inner = body.remove(CAUSE).and_then(|c| c.as_str().map(str::to_string));
        body.insert("error".into(), json!(self.message));
        let cause = crate::incidents::Cause { message: self.message.fr.clone(), cause: inner };
        let mut resp = (self.status, Json(serde_json::Value::Object(body))).into_response();
        resp.extensions_mut().insert(cause);
        resp
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db) = &e {
            if db.is_unique_violation() {
                return ApiError::conflict("already exists");
            }
        }
        tracing::error!("database error: {e}");
        ApiError::internal("database error").with_cause(e.to_string())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("internal error: {e:#}");
        ApiError::internal(e.to_string()).with_cause(format!("{e:#}"))
    }
}

impl From<crate::airzone::AirzoneError> for ApiError {
    fn from(e: crate::airzone::AirzoneError) -> Self {
        ApiError::bad_gateway(e.to_string())
    }
}
