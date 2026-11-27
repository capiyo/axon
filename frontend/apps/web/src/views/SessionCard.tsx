import type { Chat } from "../../../../packages/contracts/src";

interface SessionCardProps {
  session: Chat;
  linkedTaskCount: number;
  progress: number | null;
  active: boolean;
  onOpen: () => void;
  onLike: () => void;
  onArchive: () => void;
}

export default function SessionCard({ session, linkedTaskCount, progress, active, onOpen, onLike, onArchive }: SessionCardProps) {
  const lastMessage = session.last_message;
  const replyLabel = lastMessage?.role === "assistant" ? "Axon responded" : lastMessage?.role === "user" ? "Your prompt" : "New session";
  return (
    <button type="button" className={active ? "chat-list-item active" : "chat-list-item"} onClick={onOpen}>
      <span className="session-card-avatar">{(lastMessage?.answered_by || session.title || "A").slice(0, 1).toUpperCase()}</span>
      <span className="session-card-content">
        <span className="session-card-heading"><strong>{session.title}</strong><small>{new Date(session.updated_at).toLocaleString()}</small></span>
        <small className="session-card-agent">{lastMessage?.answered_by || "Session"}</small>
        <span className="session-card-preview">{lastMessage?.content || "No messages yet. Open this session to begin."}</span>
        {progress !== null && <span className="session-card-progress"><span style={{ width: `${progress}%` }} /></span>}
        <span className="session-card-footer">
          <small>{session.message_count} turns · {linkedTaskCount} tasks · {replyLabel}</small>
          <span className="session-card-actions">
            {session.unread_count > 0 && <span className="chat-unread-count">{session.unread_count}</span>}
            <span role="button" tabIndex={0} className="session-card-like" onClick={event => { event.stopPropagation(); onLike(); }} onKeyDown={event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); event.stopPropagation(); onLike(); } }}>{session.is_liked ? "♥" : "♡"}</span>
            <span role="button" tabIndex={0} className="session-card-archive" onClick={event => { event.stopPropagation(); onArchive(); }} onKeyDown={event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); event.stopPropagation(); onArchive(); } }}>Archive</span>
          </span>
        </span>
      </span>
    </button>
  );
}
