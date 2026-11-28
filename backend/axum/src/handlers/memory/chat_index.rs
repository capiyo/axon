pub async fn index_chat_message(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, chat_id, message_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(input): Json<IndexChatMessageInput>,
) -> ApiResult<StatusCode> {
    require_member(&state, user.user_id, workspace_id).await?;
    if input.workspace_id != workspace_id
        || input.chat_id != chat_id
        || input.message_id != message_id
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Workspace, chat, and message path identifiers must match the payload.",
        ));
    }
    coordinator(&state)?
        .index_chat_message(input)
        .await
        .map_err(|_| {
            error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Chat message must exist and have a valid Qoin vector.",
            )
        })?;
    Ok(StatusCode::CREATED)
}

