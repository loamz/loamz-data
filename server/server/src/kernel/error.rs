//! One error type for the whole server, turned into an RFC 9457 `application/problem+json` answer in one place.

use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;

/// A permission refusal, kept apart so the audit trail can record it.
#[derive(Debug)]
pub struct Denied {
    pub action: &'static str,
    pub target: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("sign in required")]
    Unauthorized,
    #[error("{0}")]
    NotFound(String),
    #[error("you do not have permission to do that")]
    Denied(Denied),
    #[error("{0}")]
    Conflict(String),
    #[error("the file is larger than the limit")]
    TooLarge,
    /// The detail is logged, never sent: it may name tables or paths.
    #[error("something went wrong on our side")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    fn status(&self) -> StatusCode {
        match self {
            Self::Invalid(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Denied(_) => StatusCode::FORBIDDEN,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        if let Self::Internal(detail) = &self {
            tracing::error!(detail, "internal error");
        }
        let status = self.status();
        let body = json!({
            "type": "about:blank",
            "title": status.canonical_reason().unwrap_or("Error"),
            "status": status.as_u16(),
            "detail": self.to_string(),
        });
        (
            status,
            [(header::CONTENT_TYPE, "application/problem+json")],
            body.to_string(),
        )
            .into_response()
    }
}

impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(format!("database: {e}"))
    }
}

impl From<loamz_lake::sql::DbError> for Error {
    fn from(e: loamz_lake::sql::DbError) -> Self {
        Self::Internal(e.to_string())
    }
}

impl From<loamz_lake::files::FileError> for Error {
    fn from(e: loamz_lake::files::FileError) -> Self {
        use loamz_lake::files::FileError as F;
        match e {
            F::BadPath(why) => Self::Invalid(format!("that path is not allowed ({why})")),
            F::NotFound => Self::NotFound("no such file".into()),
            F::TooLarge => Self::TooLarge,
            F::Io(e) => Self::Internal(format!("files: {e}")),
        }
    }
}
