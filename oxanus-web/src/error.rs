use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub(crate) struct OxanusWebError(#[allow(dead_code)] oxanus::OxanusError);

impl From<oxanus::OxanusError> for OxanusWebError {
    fn from(err: oxanus::OxanusError) -> Self {
        Self(err)
    }
}

impl IntoResponse for OxanusWebError {
    fn into_response(self) -> Response {
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}
