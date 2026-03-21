use axum::extract::State;
use axum::http::StatusCode;

use crate::AppState;

pub async fn health() -> StatusCode {
    StatusCode::OK
}

pub async fn ready(State(state): State<AppState>) -> StatusCode {
    match state.store.readiness().await {
        Ok(true) => StatusCode::OK,
        Ok(false) => StatusCode::SERVICE_UNAVAILABLE,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
