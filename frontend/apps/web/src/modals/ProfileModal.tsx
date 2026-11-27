import type { User } from "firebase/auth";
import type { Agent, Artifact, Chat, Task, Workspace } from "../../../../packages/contracts/src";
import ModalFrame from "./ModalFrame";

interface ProfileModalProps {
  user: User;
  selected: Workspace | null;
  agents: Agent[];
  chats: Chat[];
  tasks: Task[];
  artifacts: Artifact[];
  onClose: () => void;
  onSignOut: () => void;
}

export default function ProfileModal({ user, selected, agents, chats, tasks, artifacts, onClose, onSignOut }: ProfileModalProps) {
  return (
    <ModalFrame className="profile-overlay" role="dialog" ariaModal="true" ariaLabel="Profile and workspace overview">
      <div className="profile-overlay-header"><strong>Profile & workspace</strong><button type="button" onClick={onClose}>Close</button></div>
      <div className="profile-overlay-body">
        <div className="profile-identity"><div className="profile-avatar">{user.photoURL ? <img src={user.photoURL} alt="" /> : (user.displayName || user.email || "A").slice(0,1).toUpperCase()}</div><div className="profile-identity-copy"><strong>{user.displayName || user.email || "Axon user"}</strong><small>{selected?.name ?? "Personal workspace"}</small></div></div>
        <div className="profile-stat-grid">
          <div><strong>{agents.length}</strong><small>Agents</small></div><div><strong>{chats.length}</strong><small>Chats</small></div>
          <div><strong>{tasks.filter(task => task.status === "in_progress").length}</strong><small>Running</small></div><div><strong>{tasks.filter(task => task.status === "completed").length}</strong><small>Completed</small></div>
          <div><strong>{tasks.filter(task => task.status === "offered" || task.status === "acknowledged").length}</strong><small>Queued</small></div><div><strong>{artifacts.length}</strong><small>Artifacts</small></div>
        </div>
        <p className="profile-overlay-email">{user.email}</p><button type="button" className="signout-button" onClick={onSignOut}>Sign out</button>
      </div>
    </ModalFrame>
  );
}
