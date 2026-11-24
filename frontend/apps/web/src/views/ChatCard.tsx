import type { Chat } from "../../../../packages/contracts/src";

interface ChatCardProps {
  chat: Chat;
  linkedTaskCount: number;
  progress: number | null;
  active: boolean;
  onOpen: () => void;
  onLike: () => void;
  onArchive: () => void;
}

export default function ChatCard({ chat, linkedTaskCount, progress, active, onOpen, onLike, onArchive }: ChatCardProps) {
  const lastMessage = chat.last_message;
  const replyLabel = lastMessage?.role === "assistant" ? "Agent replied" : lastMessage?.role === "user" ? "Your message" : "New session";
  return (
    <button type="button" className={active ? "chat-list-item active" : "chat-list-item"} onClick={onOpen}>
      <span className="chat-card-avatar">{(lastMessage?.answered_by || chat.title || "A").slice(0, 1).toUpperCase()}</span>
      <span className="chat-card-content">
        <span className="chat-card-heading"><strong>{chat.title}</strong><small>{new Date(chat.updated_at).toLocaleString()}</small></span>
        <small className="chat-card-agent">{lastMessage?.answered_by || "Session"}</small>
        <span className="chat-card-preview">{lastMessage?.content || "No messages yet. Open this chat to begin."}</span>
        {progress !== null && <span className="chat-card-progress"><span style={{ width: `${progress}%` }} /></span>}
        <span className="chat-card-footer">
          <small>{chat.message_count} messages · {linkedTaskCount} tasks · {replyLabel}</small>
          <span className="chat-card-actions">
            {chat.unread_count > 0 && <span className="chat-unread-count">{chat.unread_count}</span>}
            <span role="button" tabIndex={0} className="chat-card-like" onClick={event => { event.stopPropagation(); onLike(); }} onKeyDown={event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); event.stopPropagation(); onLike(); } }}>{chat.is_liked ? "♥" : "♡"}</span>
            <span role="button" tabIndex={0} className="chat-card-archive" onClick={event => { event.stopPropagation(); onArchive(); }} onKeyDown={event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); event.stopPropagation(); onArchive(); } }}>Archive</span>
          </span>
        </span>
      </span>
    </button>
  );
}
