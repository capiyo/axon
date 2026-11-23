CREATE TABLE IF NOT EXISTS agent_listings (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL UNIQUE,
    workspace_id TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    title TEXT NOT NULL,
    provider TEXT NOT NULL,
    description TEXT NOT NULL,
    category TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_listings_public_recent
    ON agent_listings(updated_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS agent_listing_likes (
    listing_id TEXT NOT NULL REFERENCES agent_listings(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(listing_id, user_id)
);
CREATE TABLE IF NOT EXISTS agent_listing_follows (
    listing_id TEXT NOT NULL REFERENCES agent_listings(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(listing_id, user_id)
);
CREATE TABLE IF NOT EXISTS agent_comments (
    id TEXT PRIMARY KEY,
    listing_id TEXT NOT NULL REFERENCES agent_listings(id) ON DELETE CASCADE,
    parent_comment_id TEXT REFERENCES agent_comments(id) ON DELETE CASCADE,
    author_id TEXT NOT NULL,
    body TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'visible',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_comments_listing_created
    ON agent_comments(listing_id, created_at DESC);
CREATE TABLE IF NOT EXISTS agent_comment_likes (
    comment_id TEXT NOT NULL REFERENCES agent_comments(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(comment_id, user_id)
);
CREATE TABLE IF NOT EXISTS agent_reports (
    id TEXT PRIMARY KEY,
    listing_id TEXT NOT NULL REFERENCES agent_listings(id) ON DELETE CASCADE,
    comment_id TEXT REFERENCES agent_comments(id) ON DELETE CASCADE,
    reporter_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_reports_status_created
    ON agent_reports(status, created_at);

CREATE TABLE IF NOT EXISTS chat_read_states (
    chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    last_read_at TEXT NOT NULL,
    PRIMARY KEY(chat_id, user_id)
);
CREATE TABLE IF NOT EXISTS chat_reactions (
    chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    reaction TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(chat_id, user_id, reaction)
);
CREATE INDEX IF NOT EXISTS idx_chat_reactions_user
    ON chat_reactions(user_id, chat_id);
