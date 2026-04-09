use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub struct AppError {
    pub status: StatusCode,
    pub message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": self.message,
        });
        (self.status, axum::Json(body)).into_response()
    }
}
