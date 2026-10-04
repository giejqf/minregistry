//! The application error. Registry handlers render it as OCI error JSON
//! (`registry::error`); management handlers render it as
//! `{"error": {"code", "message"}}` through `IntoResponse`.

use std::fmt;

use axum::{
    http::{header, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::{registry::error::OciCode, storage::StorageError};

pub(crate) struct AppError {
    pub status: StatusCode,
    pub code: OciCode,
    pub api_code: &'static str,
    pub message: String,
    pub detail: Option<Value>,
    pub headers: Vec<(HeaderName, HeaderValue)>,
}

pub(crate) type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub(crate) fn new(status: StatusCode, code: OciCode, message: impl Into<String>) -> Self {
        let api_code = match status {
            StatusCode::BAD_REQUEST => "bad_request",
            StatusCode::UNAUTHORIZED => "unauthorized",
            StatusCode::FORBIDDEN => "forbidden",
            StatusCode::NOT_FOUND => "not_found",
            StatusCode::CONFLICT => "conflict",
            StatusCode::METHOD_NOT_ALLOWED => "method_not_allowed",
            StatusCode::PAYLOAD_TOO_LARGE => "payload_too_large",
            s if s.is_server_error() => "internal",
            _ => "error",
        };
        AppError { status, code, api_code, message: message.into(), detail: None, headers: Vec::new() }
    }

    pub(crate) fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, OciCode::Unauthorized, "authentication required")
    }

    pub(crate) fn denied(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, OciCode::Denied, message)
    }

    pub(crate) fn not_found(code: OciCode, message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, message)
    }

    pub(crate) fn bad_request(code: OciCode, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, OciCode::Denied, message)
    }

    /// Logs the cause and returns an opaque 500.
    pub(crate) fn internal(cause: impl fmt::Display) -> Self {
        tracing::error!(error = %cause, "internal error");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, OciCode::Unknown, "internal server error")
    }

    pub(crate) fn with_detail(mut self, detail: Value) -> Self {
        self.detail = Some(detail);
        self
    }

    pub(crate) fn with_api_code(mut self, code: &'static str) -> Self {
        self.api_code = code;
        self
    }

    pub(crate) fn with_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.push((name, value));
        self
    }

    /// Spec-shaped response for `/v2/`.
    pub(crate) fn into_oci_response(self) -> Response {
        let body = crate::registry::error::body(self.code, &self.message, self.detail.as_ref());
        let mut res = (self.status, Json(body)).into_response();
        if self.status == StatusCode::UNAUTHORIZED {
            res.headers_mut().insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Basic realm=\"minregistry\""));
        }
        for (k, v) in self.headers {
            res.headers_mut().insert(k, v);
        }
        res
    }
}

impl fmt::Debug for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.status.as_u16(), self.code.as_str(), self.message)
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Management API shape.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let mut error = json!({ "code": self.api_code, "message": self.message });
        if let Some(detail) = self.detail {
            error["detail"] = detail;
        }
        let mut res = (self.status, Json(json!({ "error": error }))).into_response();
        for (k, v) in self.headers {
            res.headers_mut().insert(k, v);
        }
        res
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::internal(format_args!("database: {e}"))
    }
}

impl From<StorageError> for AppError {
    fn from(e: StorageError) -> Self {
        AppError::internal(format_args!("storage: {e}"))
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::internal(format_args!("io: {e}"))
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::internal(format_args!("{e:#}"))
    }
}

impl From<tower_sessions::session::Error> for AppError {
    fn from(e: tower_sessions::session::Error) -> Self {
        AppError::internal(format_args!("session: {e}"))
    }
}
