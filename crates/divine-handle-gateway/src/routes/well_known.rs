use axum::extract::{Host, State};
use axum::http::StatusCode;

use super::super::{AppState, ProvisioningState};

pub async fn handler(
    State(state): State<AppState>,
    Host(host): Host,
) -> Result<String, StatusCode> {
    let host_without_port = host.split(':').next().unwrap_or(&host);
    let record = state
        .get_by_handle(host_without_port)
        .ok_or(StatusCode::NOT_FOUND)?;

    if record.provisioning_state != ProvisioningState::Ready {
        return Err(StatusCode::NOT_FOUND);
    }

    record.did.ok_or(StatusCode::NOT_FOUND)
}
