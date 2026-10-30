use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use nucleo::ErrorCanal;
use protocolo::{codigos, CuerpoError, DetalleError};

/// Error de la API: HTTP 4xx/5xx con `{"error": {"codigo", "detalle"}}`.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub codigo: String,
    pub detalle: String,
}

impl ApiError {
    pub fn new(status: StatusCode, codigo: &str, detalle: impl Into<String>) -> Self {
        Self { status, codigo: codigo.into(), detalle: detalle.into() }
    }
    pub fn parametro(detalle: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, codigos::PARAMETRO, detalle)
    }
    pub fn sin_dispositivo(id: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, codigos::SIN_DISPOSITIVO, format!("el dispositivo '{id}' no está conectado"))
    }
}

impl From<ErrorCanal> for ApiError {
    fn from(e: ErrorCanal) -> Self {
        let status = match e.codigo.as_str() {
            codigos::OCUPADO => StatusCode::CONFLICT,
            codigos::SESION_INEXISTENTE => StatusCode::NOT_FOUND,
            codigos::PARAMETRO | codigos::SIM_INVALIDA => StatusCode::BAD_REQUEST,
            codigos::TIMEOUT => StatusCode::GATEWAY_TIMEOUT,
            codigos::DESCONECTADO => StatusCode::SERVICE_UNAVAILABLE,
            codigos::SIN_PERMISO | codigos::SIN_ACCESIBILIDAD => StatusCode::PRECONDITION_FAILED,
            codigos::RED => StatusCode::BAD_GATEWAY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self { status, codigo: e.codigo, detalle: e.detalle }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(CuerpoError { error: DetalleError { codigo: self.codigo, detalle: self.detalle } }))
            .into_response()
    }
}

pub type ApiResult<T> = Result<Json<T>, ApiError>;

/// Como `Json<T>`, pero un cuerpo inválido responde con el error del contrato (`parametro`, HTTP 400).
pub struct Cuerpo<T>(pub T);

#[axum::async_trait]
impl<S, T> axum::extract::FromRequest<S> for Cuerpo<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(Cuerpo(v)),
            Err(r) => Err(ApiError::parametro(r.body_text())),
        }
    }
}
