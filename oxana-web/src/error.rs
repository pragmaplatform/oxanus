use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug)]
pub(crate) struct OxanaWebError(#[allow(dead_code)] oxana::OxanaError);

impl From<oxana::OxanaError> for OxanaWebError {
    fn from(err: oxana::OxanaError) -> Self {
        Self(err)
    }
}

impl IntoResponse for OxanaWebError {
    fn into_response(self) -> Response {
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}
