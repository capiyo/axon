use crate::{auth_middleware::AuthenticatedUser, AppState};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct ExploreStore {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExploreAgent {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub workspace_id: Uuid,
    pub owner_id: Uuid,
    pub title: String,
    pub provider: String,
    pub description: String,
    pub category: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub like_count: i64,
    pub comment_count: i64,
    pub is_liked: bool,
    pub is_following: bool,
    pub is_owner: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExploreComment {
    pub id: Uuid,
    pub listing_id: Uuid,
    pub parent_comment_id: Option<Uuid>,
    pub author_id: Uuid,
    pub author_label: &'static str,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub like_count: i64,
    pub reply_count: i64,
    pub is_liked: bool,
    pub is_mine: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub event_type: String,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub title: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy)]
pub struct PublishAgentRecord<'a> {
    pub agent_id: Uuid,
    pub workspace_id: Uuid,
    pub owner_id: Uuid,
    pub title: &'a str,
    pub provider: &'a str,
    pub description: &'a str,
    pub category: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct NotificationInput<'a> {
    pub recipient_id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub event_type: &'a str,
    pub entity_type: &'a str,
    pub entity_id: Uuid,
    pub title: &'a str,
    pub body: &'a str,
    pub actor_id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotificationPage {
    pub items: Vec<Notification>,
    pub unread_count: i64,
}

#[derive(Debug, Deserialize)]
pub struct PublishAgentInput {
    pub description: String,
    pub category: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateCommentInput {
    pub body: String,
    pub parent_comment_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct EditCommentInput {
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct ReportInput {
    pub listing_id: Uuid,
    pub comment_id: Option<Uuid>,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct ExploreQuery {
    pub sort: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ExploreApiError {
    error: String,
}

type ApiResult<T> = Result<T, (StatusCode, Json<ExploreApiError>)>;

fn error(status: StatusCode, message: &str) -> (StatusCode, Json<ExploreApiError>) {
    (
        status,
        Json(ExploreApiError {
            error: message.into(),
        }),
    )
}

impl ExploreStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn publish(
        &self,
        input: PublishAgentRecord<'_>,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        let now = Utc::now();
        let listing_id = Uuid::new_v4();
        let result = sqlx::query(
            "INSERT INTO agent_listings (id, agent_id, workspace_id, owner_id, title, provider, description, category, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(agent_id) DO UPDATE SET title = excluded.title, provider = excluded.provider,
             description = excluded.description, category = excluded.category, updated_at = excluded.updated_at
             WHERE agent_listings.owner_id = excluded.owner_id AND agent_listings.workspace_id = excluded.workspace_id"
        )
        .bind(listing_id.to_string())
        .bind(input.agent_id.to_string())
        .bind(input.workspace_id.to_string())
        .bind(input.owner_id.to_string())
        .bind(input.title)
        .bind(input.provider)
        .bind(input.description)
        .bind(input.category)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        let id: String = sqlx::query_scalar("SELECT id FROM agent_listings WHERE agent_id = ?")
            .bind(input.agent_id.to_string())
            .fetch_one(&self.pool)
            .await?;
        Ok(Uuid::parse_str(&id).ok())
    }

    pub async fn unpublish(
        &self,
        agent_id: Uuid,
        workspace_id: Uuid,
        owner_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "DELETE FROM agent_listings WHERE agent_id = ? AND workspace_id = ? AND owner_id = ?",
        )
        .bind(agent_id.to_string())
        .bind(workspace_id.to_string())
        .bind(owner_id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn list_agents(
        &self,
        user_id: Uuid,
        sort: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ExploreAgent>, sqlx::Error> {
        let order = match sort {
            "popular" => "like_count DESC, comment_count DESC, updated_at DESC",
            "discussed" => "comment_count DESC, like_count DESC, updated_at DESC",
            _ => "updated_at DESC",
        };
        let sql = format!(
            "SELECT l.id, l.agent_id, l.workspace_id, l.owner_id, l.title, l.provider, l.description, l.category, l.created_at, l.updated_at,
             (SELECT COUNT(*) FROM agent_listing_likes x WHERE x.listing_id = l.id) AS like_count,
             (SELECT COUNT(*) FROM agent_comments c WHERE c.listing_id = l.id AND c.status = 'visible') AS comment_count,
             EXISTS(SELECT 1 FROM agent_listing_likes x WHERE x.listing_id = l.id AND x.user_id = ?) AS is_liked,
             EXISTS(SELECT 1 FROM agent_listing_follows f WHERE f.listing_id = l.id AND f.user_id = ?) AS is_following,
             l.owner_id = ? AS is_owner
             FROM agent_listings l ORDER BY {order} LIMIT ? OFFSET ?"
        );
        let rows = sqlx::query(&sql)
            .bind(user_id.to_string())
            .bind(user_id.to_string())
            .bind(limit.clamp(1, 50))
            .bind(offset.max(0))
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(parse_agent).collect()
    }

    pub async fn listing_exists(&self, listing_id: Uuid) -> Result<bool, sqlx::Error> {
        let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM agent_listings WHERE id = ?")
            .bind(listing_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        Ok(exists.is_some())
    }

    pub async fn list_comments(
        &self,
        listing_id: Uuid,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ExploreComment>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT c.id, c.listing_id, c.parent_comment_id, c.author_id, c.body, c.created_at, c.updated_at,
             (SELECT COUNT(*) FROM agent_comment_likes l WHERE l.comment_id = c.id) AS like_count,
             (SELECT COUNT(*) FROM agent_comments r WHERE r.parent_comment_id = c.id AND r.status = 'visible') AS reply_count,
             EXISTS(SELECT 1 FROM agent_comment_likes l WHERE l.comment_id = c.id AND l.user_id = ?) AS is_liked,
             c.author_id = ? AS is_mine
             FROM agent_comments c WHERE c.listing_id = ? AND c.status = 'visible'
             ORDER BY c.created_at ASC, c.id ASC LIMIT ? OFFSET ?"
        )
        .bind(user_id.to_string())
        .bind(user_id.to_string())
        .bind(listing_id.to_string())
        .bind(limit.clamp(1, 100))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(parse_comment).collect()
    }

    pub async fn create_comment(
        &self,
        listing_id: Uuid,
        author_id: Uuid,
        parent_comment_id: Option<Uuid>,
        body: &str,
    ) -> Result<Option<ExploreComment>, sqlx::Error> {
        if !self.listing_exists(listing_id).await? {
            return Ok(None);
        }
        if let Some(parent_id) = parent_comment_id {
            let parent_listing: Option<String> = sqlx::query_scalar(
                "SELECT listing_id FROM agent_comments WHERE id = ? AND status = 'visible'",
            )
            .bind(parent_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
            if parent_listing.as_deref() != Some(&listing_id.to_string()) {
                return Ok(None);
            }
        }
        let now = Utc::now();
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agent_comments (id, listing_id, parent_comment_id, author_id, body, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 'visible', ?, ?)"
        )
        .bind(id.to_string())
        .bind(listing_id.to_string())
        .bind(parent_comment_id.map(|value| value.to_string()))
        .bind(author_id.to_string())
        .bind(body)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        let recipient: Option<String> = if let Some(parent_id) = parent_comment_id {
            sqlx::query_scalar("SELECT author_id FROM agent_comments WHERE id = ?")
                .bind(parent_id.to_string())
                .fetch_optional(&self.pool)
                .await?
        } else {
            sqlx::query_scalar("SELECT owner_id FROM agent_listings WHERE id = ?")
                .bind(listing_id.to_string())
                .fetch_optional(&self.pool)
                .await?
        };
        if let Some(recipient) = recipient.and_then(|value| Uuid::parse_str(&value).ok()) {
            let workspace = sqlx::query_scalar::<_, String>(
                "SELECT workspace_id FROM agent_listings WHERE id = ?",
            )
            .bind(listing_id.to_string())
            .fetch_one(&self.pool)
            .await?;
            notify_user(
                &self.pool,
                NotificationInput {
                    recipient_id: recipient,
                    workspace_id: Uuid::parse_str(&workspace).ok(),
                    event_type: if parent_comment_id.is_some() {
                        "explore.reply"
                    } else {
                        "explore.comment"
                    },
                    entity_type: "agent_listing",
                    entity_id: listing_id,
                    title: if parent_comment_id.is_some() {
                        "New reply on an agent discussion"
                    } else {
                        "New comment on your agent"
                    },
                    body,
                    actor_id: author_id,
                },
            )
            .await?;
        }
        self.get_comment(id, author_id).await
    }

    async fn get_comment(
        &self,
        comment_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<ExploreComment>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT c.id, c.listing_id, c.parent_comment_id, c.author_id, c.body, c.created_at, c.updated_at,
             (SELECT COUNT(*) FROM agent_comment_likes l WHERE l.comment_id = c.id) AS like_count,
             (SELECT COUNT(*) FROM agent_comments r WHERE r.parent_comment_id = c.id AND r.status = 'visible') AS reply_count,
             EXISTS(SELECT 1 FROM agent_comment_likes l WHERE l.comment_id = c.id AND l.user_id = ?) AS is_liked,
             c.author_id = ? AS is_mine
             FROM agent_comments c WHERE c.id = ? AND c.status = 'visible'"
        )
        .bind(user_id.to_string())
        .bind(user_id.to_string())
        .bind(comment_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(parse_comment).transpose()
    }

    pub async fn edit_comment(
        &self,
        comment_id: Uuid,
        author_id: Uuid,
        body: &str,
    ) -> Result<Option<ExploreComment>, sqlx::Error> {
        let now = Utc::now();
        let result = sqlx::query(
            "UPDATE agent_comments SET body = ?, updated_at = ? WHERE id = ? AND author_id = ? AND status = 'visible'"
        )
        .bind(body)
        .bind(now.to_rfc3339())
        .bind(comment_id.to_string())
        .bind(author_id.to_string())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.get_comment(comment_id, author_id).await
    }

    pub async fn delete_comment(
        &self,
        comment_id: Uuid,
        author_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE agent_comments SET body = '[deleted]', status = 'deleted', updated_at = ? WHERE id = ? AND author_id = ? AND status = 'visible'"
        )
        .bind(Utc::now().to_rfc3339())
        .bind(comment_id.to_string())
        .bind(author_id.to_string())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 1 {
            sqlx::query("UPDATE agent_comments SET body = '[deleted]', status = 'deleted', updated_at = ? WHERE parent_comment_id = ? AND status = 'visible'")
                .bind(Utc::now().to_rfc3339())
                .bind(comment_id.to_string())
                .execute(&self.pool)
                .await?;
        }
        Ok(result.rows_affected() == 1)
    }

    pub async fn toggle_listing_like(
        &self,
        listing_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let liked = toggle_relation(
            &self.pool,
            "agent_listing_likes",
            "listing_id",
            listing_id,
            user_id,
        )
        .await?;
        if liked {
            notify_listing_owner(
                &self.pool,
                listing_id,
                user_id,
                "explore.agent_liked",
                "Someone liked your agent",
                "Your public agent received a like.",
            )
            .await?;
        }
        Ok(liked)
    }

    pub async fn toggle_follow(
        &self,
        listing_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let following = toggle_relation(
            &self.pool,
            "agent_listing_follows",
            "listing_id",
            listing_id,
            user_id,
        )
        .await?;
        if following {
            notify_listing_owner(
                &self.pool,
                listing_id,
                user_id,
                "explore.agent_followed",
                "Someone followed your agent",
                "Your public agent received a new follower.",
            )
            .await?;
        }
        Ok(following)
    }

    pub async fn toggle_comment_like(
        &self,
        comment_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM agent_comments WHERE id = ? AND status = 'visible'")
                .bind(comment_id.to_string())
                .fetch_optional(&self.pool)
                .await?;
        if exists.is_none() {
            return Ok(false);
        }
        let liked = toggle_relation(
            &self.pool,
            "agent_comment_likes",
            "comment_id",
            comment_id,
            user_id,
        )
        .await?;
        if liked {
            let row = sqlx::query("SELECT c.author_id, c.listing_id, l.workspace_id FROM agent_comments c JOIN agent_listings l ON l.id = c.listing_id WHERE c.id = ?")
                .bind(comment_id.to_string()).fetch_one(&self.pool).await?;
            let recipient =
                Uuid::parse_str(&row.try_get::<String, _>("author_id")?).unwrap_or_default();
            let listing_id =
                Uuid::parse_str(&row.try_get::<String, _>("listing_id")?).unwrap_or_default();
            let workspace_id = Uuid::parse_str(&row.try_get::<String, _>("workspace_id")?).ok();
            notify_user(
                &self.pool,
                NotificationInput {
                    recipient_id: recipient,
                    workspace_id,
                    event_type: "explore.comment_liked",
                    entity_type: "agent_comment",
                    entity_id: comment_id,
                    title: "Someone liked your comment",
                    body: "Your Explore comment received a like.",
                    actor_id: user_id,
                },
            )
            .await?;
            let _ = listing_id;
        }
        Ok(liked)
    }

    pub async fn notify_task_event(
        &self,
        notification: NotificationInput<'_>,
    ) -> Result<(), sqlx::Error> {
        notify_user(&self.pool, notification).await
    }

    pub async fn list_notifications(
        &self,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<NotificationPage, sqlx::Error> {
        let unread_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notifications WHERE recipient_id = ? AND read_at IS NULL",
        )
        .bind(user_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        let rows = sqlx::query(
            "SELECT id, workspace_id, event_type, entity_type, entity_id, title, body, created_at, read_at
             FROM notifications WHERE recipient_id = ? ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?"
        )
        .bind(user_id.to_string()).bind(limit.clamp(1, 50)).bind(offset.max(0)).fetch_all(&self.pool).await?;
        let items = rows
            .iter()
            .map(parse_notification)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(NotificationPage {
            items,
            unread_count,
        })
    }

    pub async fn mark_notification_read(
        &self,
        notification_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("UPDATE notifications SET read_at = ? WHERE id = ? AND recipient_id = ? AND read_at IS NULL")
            .bind(Utc::now().to_rfc3339()).bind(notification_id.to_string()).bind(user_id.to_string())
            .execute(&self.pool).await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn mark_all_notifications_read(&self, user_id: Uuid) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE notifications SET read_at = ? WHERE recipient_id = ? AND read_at IS NULL",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(user_id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn report(
        &self,
        listing_id: Uuid,
        comment_id: Option<Uuid>,
        reporter_id: Uuid,
        reason: &str,
    ) -> Result<bool, sqlx::Error> {
        if !self.listing_exists(listing_id).await? {
            return Ok(false);
        }
        if let Some(comment_id) = comment_id {
            let belongs: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM agent_comments WHERE id = ? AND listing_id = ?")
                    .bind(comment_id.to_string())
                    .bind(listing_id.to_string())
                    .fetch_optional(&self.pool)
                    .await?;
            if belongs.is_none() {
                return Ok(false);
            }
        }
        sqlx::query("INSERT INTO agent_reports (id, listing_id, comment_id, reporter_id, reason, status, created_at) VALUES (?, ?, ?, ?, ?, 'pending', ?)")
            .bind(Uuid::new_v4().to_string()).bind(listing_id.to_string())
            .bind(comment_id.map(|id| id.to_string())).bind(reporter_id.to_string())
            .bind(reason).bind(Utc::now().to_rfc3339()).execute(&self.pool).await?;
        Ok(true)
    }
}

async fn toggle_relation(
    pool: &SqlitePool,
    table: &str,
    foreign_key: &str,
    target_id: Uuid,
    user_id: Uuid,
) -> Result<bool, sqlx::Error> {
    // Table and column names are selected only by the fixed internal call sites above.
    let sql = format!(
        "INSERT OR IGNORE INTO {table} ({foreign_key}, user_id, created_at) VALUES (?, ?, ?)"
    );
    let inserted = sqlx::query(&sql)
        .bind(target_id.to_string())
        .bind(user_id.to_string())
        .bind(Utc::now().to_rfc3339())
        .execute(pool)
        .await?;
    if inserted.rows_affected() == 1 {
        return Ok(true);
    }
    let sql = format!("DELETE FROM {table} WHERE {foreign_key} = ? AND user_id = ?");
    sqlx::query(&sql)
        .bind(target_id.to_string())
        .bind(user_id.to_string())
        .execute(pool)
        .await?;
    Ok(false)
}

async fn notify_user(
    pool: &SqlitePool,
    notification: NotificationInput<'_>,
) -> Result<(), sqlx::Error> {
    if notification.recipient_id.is_nil() || notification.recipient_id == notification.actor_id {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO notifications (id, recipient_id, workspace_id, event_type, entity_type, entity_id, title, body, created_at, read_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)"
    )
    .bind(Uuid::new_v4().to_string())
    .bind(notification.recipient_id.to_string())
    .bind(notification.workspace_id.map(|id| id.to_string()))
    .bind(notification.event_type)
    .bind(notification.entity_type)
    .bind(notification.entity_id.to_string())
    .bind(notification.title)
    .bind(notification.body.chars().take(220).collect::<String>())
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

async fn notify_listing_owner(
    pool: &SqlitePool,
    listing_id: Uuid,
    actor_id: Uuid,
    event_type: &str,
    title: &str,
    body: &str,
) -> Result<(), sqlx::Error> {
    let row = sqlx::query("SELECT owner_id, workspace_id FROM agent_listings WHERE id = ?")
        .bind(listing_id.to_string())
        .fetch_optional(pool)
        .await?;
    if let Some(row) = row {
        let owner = Uuid::parse_str(&row.try_get::<String, _>("owner_id")?).unwrap_or_default();
        let workspace = Uuid::parse_str(&row.try_get::<String, _>("workspace_id")?).ok();
        notify_user(
            pool,
            NotificationInput {
                recipient_id: owner,
                workspace_id: workspace,
                event_type,
                entity_type: "agent_listing",
                entity_id: listing_id,
                title,
                body,
                actor_id,
            },
        )
        .await?;
    }
    Ok(())
}

fn parse_notification(row: &sqlx::sqlite::SqliteRow) -> Result<Notification, sqlx::Error> {
    let parse_time = |key: &str| -> Result<DateTime<Utc>, sqlx::Error> {
        let value: String = row.try_get(key)?;
        DateTime::parse_from_rfc3339(&value)
            .map(|time| time.with_timezone(&Utc))
            .map_err(|_| sqlx::Error::ColumnDecode {
                index: key.into(),
                source: Box::new(std::io::Error::other("invalid timestamp")),
            })
    };
    let workspace_id: Option<String> = row.try_get("workspace_id")?;
    let read_at: Option<String> = row.try_get("read_at")?;
    let parse_optional = |value: Option<String>| {
        value.and_then(|raw| {
            DateTime::parse_from_rfc3339(&raw)
                .ok()
                .map(|time| time.with_timezone(&Utc))
        })
    };
    Ok(Notification {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?).unwrap_or_default(),
        workspace_id: workspace_id.and_then(|value| Uuid::parse_str(&value).ok()),
        event_type: row.try_get("event_type")?,
        entity_type: row.try_get("entity_type")?,
        entity_id: Uuid::parse_str(&row.try_get::<String, _>("entity_id")?).unwrap_or_default(),
        title: row.try_get("title")?,
        body: row.try_get("body")?,
        created_at: parse_time("created_at")?,
        read_at: parse_optional(read_at),
    })
}

fn parse_agent(row: &sqlx::sqlite::SqliteRow) -> Result<ExploreAgent, sqlx::Error> {
    let parse_time = |key: &str| -> Result<DateTime<Utc>, sqlx::Error> {
        let value: String = row.try_get(key)?;
        DateTime::parse_from_rfc3339(&value)
            .map(|time| time.with_timezone(&Utc))
            .map_err(|_| sqlx::Error::ColumnDecode {
                index: key.into(),
                source: Box::new(std::io::Error::other("invalid timestamp")),
            })
    };
    Ok(ExploreAgent {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?).unwrap_or_default(),
        agent_id: Uuid::parse_str(&row.try_get::<String, _>("agent_id")?).unwrap_or_default(),
        workspace_id: Uuid::parse_str(&row.try_get::<String, _>("workspace_id")?)
            .unwrap_or_default(),
        owner_id: Uuid::parse_str(&row.try_get::<String, _>("owner_id")?).unwrap_or_default(),
        title: row.try_get("title")?,
        provider: row.try_get("provider")?,
        description: row.try_get("description")?,
        category: row.try_get("category")?,
        created_at: parse_time("created_at")?,
        updated_at: parse_time("updated_at")?,
        like_count: row.try_get("like_count")?,
        comment_count: row.try_get("comment_count")?,
        is_liked: row.try_get::<i64, _>("is_liked")? != 0,
        is_following: row.try_get::<i64, _>("is_following")? != 0,
        is_owner: row.try_get::<i64, _>("is_owner")? != 0,
    })
}

fn parse_comment(row: &sqlx::sqlite::SqliteRow) -> Result<ExploreComment, sqlx::Error> {
    let parse_time = |key: &str| -> Result<DateTime<Utc>, sqlx::Error> {
        let value: String = row.try_get(key)?;
        DateTime::parse_from_rfc3339(&value)
            .map(|time| time.with_timezone(&Utc))
            .map_err(|_| sqlx::Error::ColumnDecode {
                index: key.into(),
                source: Box::new(std::io::Error::other("invalid timestamp")),
            })
    };
    let parent: Option<String> = row.try_get("parent_comment_id")?;
    Ok(ExploreComment {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?).unwrap_or_default(),
        listing_id: Uuid::parse_str(&row.try_get::<String, _>("listing_id")?).unwrap_or_default(),
        parent_comment_id: parent.and_then(|value| Uuid::parse_str(&value).ok()),
        author_id: Uuid::parse_str(&row.try_get::<String, _>("author_id")?).unwrap_or_default(),
        author_label: "Axon member",
        body: row.try_get("body")?,
        created_at: parse_time("created_at")?,
        updated_at: parse_time("updated_at")?,
        like_count: row.try_get("like_count")?,
        reply_count: row.try_get("reply_count")?,
        is_liked: row.try_get::<i64, _>("is_liked")? != 0,
        is_mine: row.try_get::<i64, _>("is_mine")? != 0,
    })
}

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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{SqliteStorage, Storage};

    #[tokio::test]
    async fn published_agents_support_public_comments_replies_and_reactions() {
        let storage = SqliteStorage::connect_in_memory()
            .await
            .expect("in-memory storage");
        let store = storage.explore_store();
        let owner_id = Uuid::new_v4();
        let visitor_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();

        let listing_id = store
            .publish(PublishAgentRecord {
                agent_id,
                workspace_id,
                owner_id,
                title: "Memory Architect",
                provider: "manual",
                description: "Organises project context and retrieves historical decisions.",
                category: "Productivity",
            })
            .await
            .expect("publish")
            .expect("listing id");

        let listings = store
            .list_agents(visitor_id, "recent", 20, 0)
            .await
            .expect("list public agents");
        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].id, listing_id);
        assert_eq!(listings[0].comment_count, 0);
        assert!(!listings[0].is_liked);

        let root = store
            .create_comment(
                listing_id,
                visitor_id,
                None,
                "The context retrieval is useful.",
            )
            .await
            .expect("create comment")
            .expect("root comment");
        let reply = store
            .create_comment(
                listing_id,
                owner_id,
                Some(root.id),
                "Thanks for sharing your feedback.",
            )
            .await
            .expect("create reply")
            .expect("reply comment");
        assert_eq!(reply.parent_comment_id, Some(root.id));

        assert!(store
            .toggle_listing_like(listing_id, visitor_id)
            .await
            .expect("like listing"));
        assert!(store
            .toggle_comment_like(root.id, visitor_id)
            .await
            .expect("like comment"));
        let comments = store
            .list_comments(listing_id, visitor_id, 50, 0)
            .await
            .expect("list comments");
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].reply_count, 1);
        assert!(comments[0].is_liked);

        let updated = store
            .list_agents(visitor_id, "popular", 20, 0)
            .await
            .expect("updated listing");
        assert_eq!(updated[0].like_count, 1);
        assert!(updated[0].is_liked);
        assert!(store
            .toggle_follow(listing_id, visitor_id)
            .await
            .expect("follow listing"));
    }

    #[tokio::test]
    async fn listing_unpublish_removes_public_discovery_record() {
        let storage = SqliteStorage::connect_in_memory()
            .await
            .expect("in-memory storage");
        let store = storage.explore_store();
        let owner_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        store
            .publish(PublishAgentRecord {
                agent_id,
                workspace_id,
                owner_id,
                title: "Agent",
                provider: "manual",
                description: "Description",
                category: "General",
            })
            .await
            .expect("publish");
        assert!(!store
            .unpublish(agent_id, workspace_id, Uuid::new_v4())
            .await
            .expect("wrong owner cannot unpublish"));
        assert!(store
            .unpublish(agent_id, workspace_id, owner_id)
            .await
            .expect("owner unpublishes"));
        assert!(store
            .list_agents(Uuid::new_v4(), "recent", 20, 0)
            .await
            .expect("list")
            .is_empty());
    }
    #[tokio::test]
    async fn community_activity_creates_owner_notifications_and_read_state() {
        let storage = SqliteStorage::connect_in_memory()
            .await
            .expect("in-memory storage");
        let store = storage.explore_store();
        let owner_id = Uuid::new_v4();
        let visitor_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        let listing_id = store
            .publish(PublishAgentRecord {
                agent_id,
                workspace_id,
                owner_id,
                title: "Agent",
                provider: "manual",
                description: "Public description",
                category: "General",
            })
            .await
            .expect("publish")
            .expect("listing");
        store
            .create_comment(listing_id, visitor_id, None, "A public comment.")
            .await
            .expect("comment");
        let page = store
            .list_notifications(owner_id, 20, 0)
            .await
            .expect("notifications");
        assert_eq!(page.unread_count, 1);
        assert_eq!(page.items[0].event_type, "explore.comment");
        assert!(store
            .mark_notification_read(page.items[0].id, owner_id)
            .await
            .expect("mark read"));
        let read_page = store
            .list_notifications(owner_id, 20, 0)
            .await
            .expect("read notifications");
        assert_eq!(read_page.unread_count, 0);
    }
}
