pub async fn list_agents(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Query(query): Query<ExploreQuery>,
) -> ApiResult<Json<Vec<ExploreAgent>>> {
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Explore storage is unavailable.",
        )
    })?;
    let items = store
        .list_agents(
            user.user_id,
            query.sort.as_deref().unwrap_or("recent"),
            query.limit.unwrap_or(20),
            query.offset.unwrap_or(0),
        )
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load Explore agents.",
            )
        })?;
    Ok(Json(items))
}

pub async fn get_agent(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(listing_id): Path<Uuid>,
) -> ApiResult<Json<ExploreAgent>> {
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Explore storage is unavailable.",
        )
    })?;
    let listing = store
        .get_agent(listing_id, user.user_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load agent details.",
            )
        })?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Agent listing not found."))?;
    Ok(Json(listing))
}

pub async fn publish_agent(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<PublishAgentInput>,
) -> ApiResult<(StatusCode, Json<Uuid>)> {
    if !state
        .is_workspace_member(user.user_id, workspace_id)
        .await
        .unwrap_or(false)
    {
        return Err(error(
            StatusCode::NOT_FOUND,
            "Workspace or agent not found.",
        ));
    }
    let agent = state
        .agents
        .read()
        .await
        .get(&agent_id)
        .cloned()
        .filter(|agent| agent.workspace_id == workspace_id)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Workspace or agent not found."))?;
    let description = input.description.trim();
    let category = input.category.trim();
    if description.is_empty()
        || description.len() > 4000
        || category.is_empty()
        || category.len() > 80
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Description must be 1–4000 characters and category 1–80 characters.",
        ));
    }
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Explore storage is unavailable.",
        )
    })?;
    let id = store
        .publish(PublishAgentRecord {
            agent_id: agent.id,
            workspace_id,
            owner_id: user.user_id,
            title: &agent.display_name,
            provider: &agent.provider,
            description,
            category,
        })
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not publish this agent.",
            )
        })?
        .ok_or_else(|| {
            error(
                StatusCode::CONFLICT,
                "This agent is already published by another workspace owner.",
            )
        })?;
    Ok((StatusCode::CREATED, Json(id)))
}

pub async fn unpublish_agent(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    if !state
        .is_workspace_member(user.user_id, workspace_id)
        .await
        .unwrap_or(false)
    {
        return Err(error(
            StatusCode::NOT_FOUND,
            "Workspace or agent not found.",
        ));
    }
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Explore storage is unavailable.",
        )
    })?;
    if store
        .unpublish(agent_id, workspace_id, user.user_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not unpublish this agent.",
            )
        })?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(error(StatusCode::NOT_FOUND, "Published agent not found."))
    }
}

pub async fn list_comments(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(listing_id): Path<Uuid>,
    Query(query): Query<ExploreQuery>,
) -> ApiResult<Json<Vec<ExploreComment>>> {
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Explore storage is unavailable.",
        )
    })?;
    if !store.listing_exists(listing_id).await.map_err(|_| {
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not load agent listing.",
        )
    })? {
        return Err(error(StatusCode::NOT_FOUND, "Agent listing not found."));
    }
    let comments = store
        .list_comments(
            listing_id,
            user.user_id,
            query.limit.unwrap_or(50),
            query.offset.unwrap_or(0),
        )
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load comments.",
            )
        })?;
    Ok(Json(comments))
}

pub async fn create_comment(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(listing_id): Path<Uuid>,
    Json(input): Json<CreateCommentInput>,
) -> ApiResult<(StatusCode, Json<ExploreComment>)> {
    let body = input.body.trim();
    if body.is_empty() || body.len() > 5000 {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Comment must contain 1–5000 characters.",
        ));
    }
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Explore storage is unavailable.",
        )
    })?;
    let comment = store
        .create_comment(listing_id, user.user_id, input.parent_comment_id, body)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not create comment.",
            )
        })?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                "Agent listing or parent comment not found.",
            )
        })?;
    Ok((StatusCode::CREATED, Json(comment)))
}

pub async fn edit_comment(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(comment_id): Path<Uuid>,
    Json(input): Json<EditCommentInput>,
) -> ApiResult<Json<ExploreComment>> {
    let body = input.body.trim();
    if body.is_empty() || body.len() > 5000 {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Comment must contain 1–5000 characters.",
        ));
    }
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Explore storage is unavailable.",
        )
    })?;
    store
        .edit_comment(comment_id, user.user_id, body)
        .await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Could not edit comment."))?
        .map(Json)
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                "Comment not found or not owned by caller.",
            )
        })
}

pub async fn delete_comment(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(comment_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Explore storage is unavailable.",
        )
    })?;
    if store
        .delete_comment(comment_id, user.user_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not delete comment.",
            )
        })?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(error(
            StatusCode::NOT_FOUND,
            "Comment not found or not owned by caller.",
        ))
    }
}

pub async fn like_listing(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(listing_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Explore storage is unavailable.",
        )
    })?;
    if !store
        .listing_exists(listing_id)
        .await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Could not load listing."))?
    {
        return Err(error(StatusCode::NOT_FOUND, "Agent listing not found."));
    }
    let liked = store
        .toggle_listing_like(listing_id, user.user_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not update reaction.",
            )
        })?;
    Ok(Json(serde_json::json!({ "liked": liked })))
}

pub async fn follow_listing(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(listing_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Explore storage is unavailable.",
        )
    })?;
    if !store
        .listing_exists(listing_id)
        .await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Could not load listing."))?
    {
        return Err(error(StatusCode::NOT_FOUND, "Agent listing not found."));
    }
    let following = store
        .toggle_follow(listing_id, user.user_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not update follow.",
            )
        })?;
    Ok(Json(serde_json::json!({ "following": following })))
}

pub async fn like_comment(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(comment_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Explore storage is unavailable.",
        )
    })?;
    let liked = store
        .toggle_comment_like(comment_id, user.user_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not update comment reaction.",
            )
        })?;
    Ok(Json(serde_json::json!({ "liked": liked })))
}

pub async fn report_content(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Json(input): Json<ReportInput>,
) -> ApiResult<StatusCode> {
    let reason = input.reason.trim();
    if reason.is_empty() || reason.len() > 1000 {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Report reason must contain 1–1000 characters.",
        ));
    }
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Explore storage is unavailable.",
        )
    })?;
    if store
        .report(input.listing_id, input.comment_id, user.user_id, reason)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not submit report.",
            )
        })?
    {
        Ok(StatusCode::CREATED)
    } else {
        Err(error(StatusCode::NOT_FOUND, "Reported content not found."))
    }
}

pub async fn list_notifications(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Query(query): Query<ExploreQuery>,
) -> ApiResult<Json<NotificationPage>> {
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Notification storage is unavailable.",
        )
    })?;
    let page = store
        .list_notifications(
            user.user_id,
            query.limit.unwrap_or(30),
            query.offset.unwrap_or(0),
        )
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load notifications.",
            )
        })?;
    Ok(Json(page))
}

pub async fn mark_notification_read(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(notification_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Notification storage is unavailable.",
        )
    })?;
    if store
        .mark_notification_read(notification_id, user.user_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not mark notification as read.",
            )
        })?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(error(
            StatusCode::NOT_FOUND,
            "Notification not found or already read.",
        ))
    }
}

pub async fn mark_all_notifications_read(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
) -> ApiResult<Json<serde_json::Value>> {
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Notification storage is unavailable.",
        )
    })?;
    let updated = store
        .mark_all_notifications_read(user.user_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not mark notifications as read.",
            )
        })?;
    Ok(Json(serde_json::json!({ "updated": updated })))
}

pub async fn list_reports(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(listing_id): Path<Uuid>,
    Query(query): Query<ExploreQuery>,
) -> ApiResult<Json<Vec<ModerationReport>>> {
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Moderation storage is unavailable.",
        )
    })?;
    let reports = store
        .list_reports(
            listing_id,
            user.user_id,
            query.limit.unwrap_or(50),
            query.offset.unwrap_or(0),
        )
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load moderation reports.",
            )
        })?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                "Listing not found or caller is not its publisher.",
            )
        })?;
    Ok(Json(reports))
}

pub async fn resolve_report(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(report_id): Path<Uuid>,
    Json(input): Json<ResolveReportInput>,
) -> ApiResult<Json<serde_json::Value>> {
    let store = state.explore.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Moderation storage is unavailable.",
        )
    })?;
    let status = store
        .resolve_report(report_id, user.user_id, input.action.trim())
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not resolve report.",
            )
        })?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                "Pending report not found or caller is not its publisher.",
            )
        })?;
    Ok(Json(serde_json::json!({ "status": status })))
}

