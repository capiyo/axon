#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::SqliteStorage;

    #[tokio::test]
    async fn published_agents_support_public_comments_replies_and_reactions() {
        let storage = SqliteStorage::connect_in_memory()
            .await
            .expect("in-memory storage");
        let store = ExploreStore::new(storage.sqlite_pool());
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
        let store = ExploreStore::new(storage.sqlite_pool());
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
        let store = ExploreStore::new(storage.sqlite_pool());
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
    #[tokio::test]
    async fn listing_owner_can_review_reports_and_hide_reported_comments() {
        let storage = SqliteStorage::connect_in_memory()
            .await
            .expect("in-memory storage");
        let store = ExploreStore::new(storage.sqlite_pool());
        let owner_id = Uuid::new_v4();
        let reporter_id = Uuid::new_v4();
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
        let comment = store
            .create_comment(listing_id, reporter_id, None, "Spam comment.")
            .await
            .expect("comment")
            .expect("visible comment");
        assert!(store
            .report(listing_id, Some(comment.id), reporter_id, "Spam or abuse.")
            .await
            .expect("report"));
        let reports = store
            .list_reports(listing_id, owner_id, 20, 0)
            .await
            .expect("reports")
            .expect("owner access");
        assert_eq!(reports.len(), 1);
        assert!(store
            .list_reports(listing_id, reporter_id, 20, 0)
            .await
            .expect("non-owner reports")
            .is_none());
        assert_eq!(
            store
                .resolve_report(reports[0].id, owner_id, "hide_comment")
                .await
                .expect("resolve")
                .as_deref(),
            Some("actioned")
        );
        let comments = store
            .list_comments(listing_id, owner_id, 50, 0)
            .await
            .expect("comments");
        assert!(comments.is_empty());
    }
}

