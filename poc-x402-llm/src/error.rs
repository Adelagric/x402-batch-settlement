use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("x402: {0}")]
    X402(#[from] x402::Error),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("upstream failed: {0}")]
    Upstream(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let code = match self {
            AppError::BadRequest(_) | AppError::X402(_) => StatusCode::BAD_REQUEST,
            AppError::Upstream(_) => StatusCode::BAD_GATEWAY,
        };
        (code, self.to_string()).into_response()
    }
}
