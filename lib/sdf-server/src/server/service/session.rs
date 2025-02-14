use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use dal::{
    KeyPairError, StandardModelError, TransactionsError, UserError, UserPk, WorkspaceError,
    WorkspacePk,
};
use thiserror::Error;

use crate::server::state::AppState;

pub mod auth_connect;
pub mod load_workspace;
pub mod restore_authentication;

#[remain::sorted]
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("auth api error: {0}")]
    AuthApiError(String),
    #[error(transparent)]
    ContextTransactions(#[from] TransactionsError),
    #[error("Invalid user: {0}")]
    InvalidUser(UserPk),
    #[error("Invalid workspace: {0}")]
    InvalidWorkspace(WorkspacePk),
    #[error("json serialize failed")]
    JSONSerialize(#[from] serde_json::Error),
    #[error(transparent)]
    KeyPair(#[from] KeyPairError),
    #[error("login failed")]
    LoginFailed,
    #[error(transparent)]
    Nats(#[from] si_data_nats::NatsError),
    #[error(transparent)]
    Pg(#[from] si_data_pg::PgError),
    #[error("http error: {0}")]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    StandardModel(#[from] StandardModelError),
    #[error("user error: {0}")]
    User(#[from] UserError),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
}

pub type SessionResult<T> = std::result::Result<T, SessionError>;

impl IntoResponse for SessionError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            SessionError::LoginFailed => (StatusCode::CONFLICT, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(
            serde_json::json!({ "error": { "message": error_message, "code": 42, "statusCode": status.as_u16() } }),
        );

        (status, body).into_response()
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/connect", post(auth_connect::auth_connect))
        .route(
            "/restore_authentication",
            get(restore_authentication::restore_authentication),
        )
        .route("/load_workspace", get(load_workspace::load_workspace))
}
