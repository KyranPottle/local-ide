use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use prompt::{PromptInput, context_report};
use protocol::{
    ApiError, CreateSession, EngineStatus, ForkSession, ModelInfo, Session, SessionDetail,
    SessionOrigin, ToolName, UpdateSession,
};
use serde::Deserialize;
use session_store::{NewSession, SessionUpdate, StoreError};

use crate::Engine;
use crate::access::WEBSOCKET_PATH;
use crate::connection;
use crate::run::agent_kind;
use crate::store::StoreAccessError;

const BODY_LIMIT: usize = 1024 * 1024;
const FIELD_LIMIT: usize = 200;

type Shared = State<Arc<Engine>>;

pub fn router(engine: Arc<Engine>) -> axum::Router {
    Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/status", get(status))
        .route("/api/models", get(models))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route(
            "/api/sessions/{id}",
            get(session_detail)
                .patch(update_session)
                .delete(delete_session),
        )
        .route("/api/sessions/{id}/fork", post(fork_session))
        .route(WEBSOCKET_PATH, get(connection::upgrade))
        .route("/api/{*path}", any(unknown_route))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(engine)
}

pub fn api_error(status: StatusCode, message: &str) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            message: message.to_owned(),
        }),
    )
}

enum Failure {
    BadRequest(String),
    SessionNotFound,
    RouteNotFound,
    Conflict,
    Internal,
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        match self {
            Self::BadRequest(message) => api_error(StatusCode::BAD_REQUEST, &message),
            Self::SessionNotFound => api_error(StatusCode::NOT_FOUND, "session not found"),
            Self::RouteNotFound => api_error(StatusCode::NOT_FOUND, "not found"),
            Self::Conflict => api_error(StatusCode::CONFLICT, "the session has the active run"),
            Self::Internal => api_error(StatusCode::INTERNAL_SERVER_ERROR, "storage error"),
        }
        .into_response()
    }
}

impl From<StoreAccessError> for Failure {
    fn from(error: StoreAccessError) -> Self {
        match error {
            StoreAccessError::Store(StoreError::SessionNotFound(_)) => Self::SessionNotFound,
            StoreAccessError::Store(error @ StoreError::InvalidSequence { .. }) => {
                Self::BadRequest(error.to_string())
            }
            error => {
                tracing::error!(%error, "session store request failed");
                Self::Internal
            }
        }
    }
}

impl From<JsonRejection> for Failure {
    fn from(rejection: JsonRejection) -> Self {
        Self::BadRequest(rejection.body_text())
    }
}

impl From<QueryRejection> for Failure {
    fn from(rejection: QueryRejection) -> Self {
        Self::BadRequest(rejection.body_text())
    }
}

fn check_field(name: &str, value: &str) -> Result<(), Failure> {
    if value.trim().is_empty() || value.chars().count() > FIELD_LIMIT {
        return Err(Failure::BadRequest(format!(
            "{name} must be non-empty and at most {FIELD_LIMIT} characters"
        )));
    }
    Ok(())
}

async fn unknown_route() -> Failure {
    Failure::RouteNotFound
}

async fn status(State(engine): Shared) -> Json<EngineStatus> {
    Json(EngineStatus {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        active_run: engine.run_lock.active_session(),
    })
}

async fn models(State(engine): Shared) -> Json<Vec<ModelInfo>> {
    Json(engine.catalog.models(&engine.inference).await)
}

#[derive(Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

async fn list_sessions(
    State(engine): Shared,
    query: Result<Query<SearchQuery>, QueryRejection>,
) -> Result<Json<Vec<Session>>, Failure> {
    let Query(SearchQuery { q }) = query?;
    if q.as_ref().is_some_and(|q| q.chars().count() > FIELD_LIMIT) {
        return Err(Failure::BadRequest(format!(
            "q must be at most {FIELD_LIMIT} characters"
        )));
    }
    let sessions = engine
        .store
        .call(move |store| store.list_sessions(q.as_deref()))
        .await?;
    Ok(Json(sessions))
}

async fn create_session(
    State(engine): Shared,
    body: Result<Json<CreateSession>, JsonRejection>,
) -> Result<Json<Session>, Failure> {
    let Json(request) = body?;
    check_field("workspace", &request.workspace)?;
    check_field("model", &request.model)?;
    let session = engine
        .store
        .call(move |store| {
            store.create_session(NewSession {
                workspace: request.workspace,
                model: request.model,
                mode: request.mode,
                origin: SessionOrigin::Root,
            })
        })
        .await?;
    Ok(Json(session))
}

async fn session_detail(
    State(engine): Shared,
    Path(id): Path<String>,
) -> Result<Json<SessionDetail>, Failure> {
    let (session, events) = engine
        .store
        .call(move |store| Ok((store.session(&id)?, store.events(&id)?)))
        .await?;
    let profile = engine.catalog.profile(&session.model);
    let workspace_tools: Vec<ToolName> = ToolName::ALL
        .into_iter()
        .filter(|tool| tool.is_workspace_tool())
        .collect();
    let delegate_targets = engine
        .catalog
        .delegate_targets(&engine.inference, &session.model)
        .await;
    let context = context_report(&PromptInput {
        profile: &profile,
        mode: session.mode,
        capabilities: &workspace_tools,
        agent_kind: agent_kind(&session.origin),
        events: &events,
        delegate_targets: &delegate_targets,
        token_scale: engine.token_scale(&profile.id),
    });
    Ok(Json(SessionDetail {
        session,
        events,
        context,
    }))
}

async fn update_session(
    State(engine): Shared,
    Path(id): Path<String>,
    body: Result<Json<UpdateSession>, JsonRejection>,
) -> Result<Json<Session>, Failure> {
    let Json(request) = body?;
    if let Some(model) = &request.model {
        check_field("model", model)?;
    }
    if request
        .title
        .as_ref()
        .is_some_and(|title| title.chars().count() > FIELD_LIMIT)
    {
        return Err(Failure::BadRequest(format!(
            "title must be at most {FIELD_LIMIT} characters"
        )));
    }
    let run_lock = engine.run_lock.clone();
    let update = SessionUpdate {
        title: request.title,
        model: request.model,
        mode: request.mode,
    };
    engine
        .store
        .call(move |store| {
            run_lock
                .unless_active(&id, || store.update_session(&id, update))
                .transpose()
        })
        .await?
        .map(Json)
        .ok_or(Failure::Conflict)
}

async fn delete_session(
    State(engine): Shared,
    Path(id): Path<String>,
) -> Result<StatusCode, Failure> {
    let run_lock = engine.run_lock.clone();
    engine
        .store
        .call(move |store| {
            run_lock
                .unless_active(&id, || store.delete_session(&id))
                .transpose()
        })
        .await?
        .map(|()| StatusCode::NO_CONTENT)
        .ok_or(Failure::Conflict)
}

async fn fork_session(
    State(engine): Shared,
    Path(id): Path<String>,
    body: Result<Json<ForkSession>, JsonRejection>,
) -> Result<Json<Session>, Failure> {
    let Json(ForkSession { seq }) = body?;
    let fork = engine
        .store
        .call(move |store| store.fork_session(&id, seq))
        .await?;
    Ok(Json(fork))
}
