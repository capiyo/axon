use crate::state::AppState;
use axum::{
    body::Body,
    extract::State,
    http::{header::AUTHORIZATION, Request, Response, StatusCode},
    middleware::Next,
};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: Uuid,
    pub firebase_uid: String,
    pub project_id: String,
    pub personal_workspace_id: Uuid,
    pub is_platform_admin: bool,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedAgent {
    pub credential_id: Uuid,
    pub agent_id: Uuid,
    pub workspace_id: Uuid,
    pub owner_user_id: Uuid,
}

fn platform_admin_allowed(firebase_uid: &str) -> bool {
    std::env::var("AXON_PLATFORM_ADMIN_UIDS")
        .ok()
        .is_some_and(|uids| uids.split(',').any(|uid| uid.trim() == firebase_uid))
}

pub async fn authenticate(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response<Body> {
    if matches!(request.uri().path(), "/health" | "/ready") {
        return next.run(request).await;
    }

    let Some(value) = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return response(StatusCode::UNAUTHORIZED);
    };
    let Some(token) = value.strip_prefix("Bearer ") else {
        return response(StatusCode::UNAUTHORIZED);
    };
    if token.starts_with("axn_") {
        let segments: Vec<&str> = request.uri().path().split('/').collect();
        let Some(workspace_text) = segments.get(4) else {
            return response(StatusCode::UNAUTHORIZED);
        };
        let Some(agent_text) = segments.get(6) else {
            return response(StatusCode::UNAUTHORIZED);
        };
        let allowed_agent_endpoint = segments.get(3) == Some(&"workspaces")
            && segments.get(5) == Some(&"agents")
            && segments.get(7).is_some_and(|endpoint| {
                *endpoint == "memory-documents"
                    || *endpoint == "memory-sessions"
                    || *endpoint == "memory-context"
            });
        if !allowed_agent_endpoint {
            return response(StatusCode::FORBIDDEN);
        }
        let (Ok(request_workspace_id), Ok(request_agent_id)) =
            (Uuid::parse_str(workspace_text), Uuid::parse_str(agent_text))
        else {
            return response(StatusCode::NOT_FOUND);
        };
        let credential = match state.authenticate_agent_credential(token).await {
            Ok(Some(credential)) => credential,
            Ok(None) => return response(StatusCode::UNAUTHORIZED),
            Err(_) => return response(StatusCode::INTERNAL_SERVER_ERROR),
        };
        if credential.workspace_id != request_workspace_id
            || credential.agent_id != request_agent_id
        {
            return response(StatusCode::FORBIDDEN);
        }
        let is_platform_admin = platform_admin_allowed(&credential.firebase_uid);
        let user = Arc::new(AuthenticatedUser {
            user_id: credential.owner_user_id,
            firebase_uid: credential.firebase_uid,
            project_id: credential.firebase_project_id,
            personal_workspace_id: credential.personal_workspace_id,
            is_platform_admin,
        });
        request.extensions_mut().insert(user);
        request
            .extensions_mut()
            .insert(Arc::new(AuthenticatedAgent {
                credential_id: credential.credential_id,
                agent_id: credential.agent_id,
                workspace_id: credential.workspace_id,
                owner_user_id: credential.owner_user_id,
            }));
        return next.run(request).await;
    }

    let Some(verifier) = state.firebase_verifier.as_ref() else {
        return response(StatusCode::SERVICE_UNAVAILABLE);
    };
    let identity = match verifier.verify(token).await {
        Ok(identity) => identity,
        Err(_) => return response(StatusCode::UNAUTHORIZED),
    };
    let account = match state
        .ensure_firebase_user(&identity.project_id, &identity.uid)
        .await
    {
        Ok(account) => account,
        Err(_) => return response(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let is_platform_admin = platform_admin_allowed(&identity.uid);
    let user = Arc::new(AuthenticatedUser {
        user_id: account.user_id,
        firebase_uid: identity.uid,
        project_id: identity.project_id,
        personal_workspace_id: account.workspace.id,
        is_platform_admin,
    });
    state
        .workspaces
        .write()
        .await
        .entry(account.workspace.id)
        .or_insert(account.workspace.clone());

    let path: Vec<&str> = request.uri().path().split('/').collect();
    if path.get(3) == Some(&"workspaces") {
        if let Some(workspace_text) = path.get(4) {
            let Ok(workspace_id) = Uuid::parse_str(workspace_text) else {
                return response(StatusCode::NOT_FOUND);
            };
            let member = state
                .is_workspace_member(user.user_id, workspace_id)
                .await
                .unwrap_or(false);
            if !member {
                return response(StatusCode::NOT_FOUND);
            }
        }
    } else if path.get(3) == Some(&"tasks") {
        if let Some(task_text) = path.get(4) {
            let Ok(task_id) = Uuid::parse_str(task_text) else {
                return response(StatusCode::NOT_FOUND);
            };
            let workspace_id = state
                .tasks
                .read()
                .await
                .get(&task_id)
                .map(|task| task.workspace_id);
            let Some(workspace_id) = workspace_id else {
                return response(StatusCode::NOT_FOUND);
            };
            let member = state
                .is_workspace_member(user.user_id, workspace_id)
                .await
                .unwrap_or(false);
            if !member {
                return response(StatusCode::NOT_FOUND);
            }
        }
    } else if path.get(3) == Some(&"sim") && !state.simulator_enabled {
        return response(StatusCode::NOT_FOUND);
    }

    request.extensions_mut().insert(user);
    next.run(request).await
}

fn response(status: StatusCode) -> Response<Body> {
    let body = match status {
        StatusCode::UNAUTHORIZED => r#"{\"error\":\"Authentication required.\"}"#,
        StatusCode::NOT_FOUND => r#"{\"error\":\"Resource not found.\"}"#,
        StatusCode::SERVICE_UNAVAILABLE => r#"{\"error\":\"Authentication is not configured.\"}"#,
        _ => r#"{\"error\":\"The request could not be completed.\"}"#,
    };
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );
    response
}
