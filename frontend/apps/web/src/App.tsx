import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { auth } from "./firebase";

import type {
  Agent, Artifact, Health, Task, Workspace, Chat, ChatDetail, ChatMessage,
  ProjectMemoryEntry, QoinVectorSnapshot, HandoverContext, ProgressiveMemoryContext,
  QoinMemoryIndexReceipt, QoinChatMessageIndexReceipt
} from "../../../packages/contracts/src";


const API_BASE_URL = (import.meta.env.VITE_AXON_API_URL ?? "").replace(/\/$/, "");

const api = async <T,>(path: string, init?: RequestInit): Promise<T> => {
  const token = await auth?.currentUser?.getIdToken();
  const response = await fetch(`${API_BASE_URL}${path}`, {
    ...init,
    headers: { "Content-Type": "application/json", ...(token ? { Authorization: `Bearer ${token}` } : {}), ...init?.headers }
  });
  if (!response.ok) {
    const body = await response.json().catch(() => ({ error: response.statusText }));
    throw new Error(body.error ?? "Request failed");
  }
  return response.json() as Promise<T>;
};

function WorkspaceApp() {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [selected, setSelected] = useState<Workspace | null>(null);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [chats, setChats] = useState<Chat[]>([]);
  const [activeChat, setActiveChat] = useState<string>("");
  const [chatDetail, setChatDetail] = useState<ChatDetail | null>(null);
  const [chatTitle, setChatTitle] = useState("");
  const [messageDraft, setMessageDraft] = useState("");
  const [memoryDraft, setMemoryDraft] = useState("");
  const [memoryType, setMemoryType] = useState<ProjectMemoryEntry["type"]>("note");
  const [editingMemoryId, setEditingMemoryId] = useState("");
  const [editMemoryDraft, setEditMemoryDraft] = useState("");
  const [name, setName] = useState("");
  const [agentName, setAgentName] = useState("");
  const [provider, setProvider] = useState("claude");
  const [artifactTitle, setArtifactTitle] = useState("");
  const [artifactContent, setArtifactContent] = useState("");
  const [taskTitle, setTaskTitle] = useState("");
  const [taskObjective, setTaskObjective] = useState("");
  const [assignee, setAssignee] = useState("");
  const [selectedArtifactIds, setSelectedArtifactIds] = useState<string[]>([]);
  const [responseDrafts, setResponseDrafts] = useState<Record<string, string>>({});
  const [health, setHealth] = useState<Health | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [qoinSymbol, setQoinSymbol] = useState("BTCUSDT");
  const [qoinVector, setQoinVector] = useState<QoinVectorSnapshot | null>(null);
  const [handoverAgentId, setHandoverAgentId] = useState("");
  const [handoverTaskId, setHandoverTaskId] = useState("");
  const [handover, setHandover] = useState<HandoverContext | null>(null);
  const [progressiveContext, setProgressiveContext] = useState<ProgressiveMemoryContext | null>(null);
  const [qoinNotice, setQoinNotice] = useState("");
  const [activeTab, setActiveTab] = useState<"chats" | "explore">("chats");
  const [menuOpen, setMenuOpen] = useState(false);

  async function refreshWorkspaces() {
    const [items, status] = await Promise.all([
      api<Workspace[]>("/api/v1/workspaces"),
      api<Health>("/health")
    ]);
    setWorkspaces(items);
    setSelected(current => current ?? items[0] ?? null);
    setHealth(status);
    setError("");
  }

  async function refreshWorkspace(workspace: Workspace) {
    const [agentItems, artifactItems, taskItems] = await Promise.all([
      api<Agent[]>(`/api/v1/workspaces/${workspace.id}/agents`),
      api<Artifact[]>(`/api/v1/workspaces/${workspace.id}/artifacts`),
      api<Task[]>(`/api/v1/workspaces/${workspace.id}/tasks`)
    ]);
    setAgents(agentItems);
    setArtifacts(artifactItems);
    setTasks(taskItems);
    const chatItems = await api<Chat[]>(`/api/v1/workspaces/${workspace.id}/chats`);
    setChats(chatItems);
    setActiveChat(current => current && chatItems.some(chat => chat.id === current) ? current : chatItems[0]?.id ?? "");
    setAssignee(current => agentItems.some(agent => agent.id === current) ? current : agentItems[0]?.id ?? "");
    setHandoverAgentId(current => agentItems.some(agent => agent.id === current) ? current : agentItems[0]?.id ?? "");
  }

  useEffect(() => {
    void refreshWorkspaces().catch(cause => setError(cause instanceof Error ? cause.message : "Could not reach the Axon API."));
  }, []);

  useEffect(() => {
    if (selected) {
      setQoinVector(null);
      setHandover(null);
      setProgressiveContext(null);
      setQoinNotice("");
      setSelectedArtifactIds([]);
      setResponseDrafts({});
      void refreshWorkspace(selected).catch(cause => setError(cause instanceof Error ? cause.message : "Could not load workspace."));
    } else {
      setAgents([]);
      setArtifacts([]);
      setTasks([]);
    }
  }, [selected?.id]);

  useEffect(() => {
    if (!selected || !activeChat) { setChatDetail(null); return; }
    api<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`)
      .then(setChatDetail)
      .catch(cause => setError(cause instanceof Error ? cause.message : "Could not load chat."));
  }, [selected?.id, activeChat]);

  async function createChat(event: FormEvent<HTMLFormElement>) {
    if (!selected) return;
    await runForm(event, async () => {
      const chat = await api<Chat>(`/api/v1/workspaces/${selected.id}/chats`, {
        method: "POST", body: JSON.stringify({ title: chatTitle.trim() })
      });
      setChatTitle("");
      setChats(current => [chat, ...current]);
      setActiveChat(chat.id);
    });
  }

  async function archiveChat() {
    if (!selected || !activeChat) return;
    setBusy(true);
    try {
      await api(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/archive`, { method: "PATCH" });
      setActiveChat("");
      setChatDetail(null);
      setChats(await api<Chat[]>(`/api/v1/workspaces/${selected.id}/chats`));
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Could not archive chat."); }
    finally { setBusy(false); }
  }

  async function sendChatMessage(event: FormEvent<HTMLFormElement>) {
    if (!selected || !activeChat || !messageDraft.trim()) return;
    await runForm(event, async () => {
      await api<ChatMessage>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/messages`, {
        method: "POST", body: JSON.stringify({ role: "user", content: messageDraft.trim() })
      });
      setMessageDraft("");
      const [detail, chatItems] = await Promise.all([
        api<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`),
        api<Chat[]>(`/api/v1/workspaces/${selected.id}/chats`)
      ]);
      setChatDetail(detail);
      setChats(chatItems);
    });
  }

  async function proposeMemory(event: FormEvent<HTMLFormElement>) {
    if (!selected || !activeChat || !memoryDraft.trim()) return;
    await runForm(event, async () => {
      await api<ProjectMemoryEntry>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/memory`, {
        method: "POST", body: JSON.stringify({ type: memoryType, text: memoryDraft.trim() })
      });
      setMemoryDraft("");
      setChatDetail(await api<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`));
    });
  }

  async function saveMemoryEdit(entry: ProjectMemoryEntry) {
    if (!selected || !activeChat || !editMemoryDraft.trim()) return;
    setBusy(true);
    try {
      await api<ProjectMemoryEntry>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/memory/${entry.id}/edit`, {
        method: "POST", body: JSON.stringify({ text: editMemoryDraft.trim(), reason: entry.reason })
      });
      setEditingMemoryId("");
      setEditMemoryDraft("");
      setChatDetail(await api<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`));
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Memory edit failed."); }
    finally { setBusy(false); }
  }

  async function reviewMemory(entry: ProjectMemoryEntry, action: "approve" | "reject" | "delete") {
    if (!selected || !activeChat) return;
    setBusy(true);
    setError("");
    try {
      await api<ProjectMemoryEntry>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/memory/${entry.id}/${action}`, { method: "POST" });
      setChatDetail(await api<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Memory update failed.");
    } finally { setBusy(false); }
  }


  async function loadQoinVector() {
    if (!selected || !qoinSymbol.trim()) return;
    setBusy(true);
    setError("");
    setQoinNotice("");
    try {
      const snapshot = await api<QoinVectorSnapshot>(
        `/api/v1/workspaces/${selected.id}/memory/qoin-vector?symbol=${encodeURIComponent(qoinSymbol.trim())}`
      );
      setQoinVector(snapshot);
      setQoinNotice(`Loaded ${snapshot.symbol} · ${snapshot.vector.model_version}`);
      setHandover(null);
      setProgressiveContext(null);
    } catch (cause) {
      setQoinVector(null);
      setError(cause instanceof Error ? cause.message : "Could not load the Qoin market vector.");
    } finally { setBusy(false); }
  }

  async function indexChatMessageWithQoin(message: ChatMessage) {
    if (!selected || !activeChat || !qoinSymbol.trim()) return;
    setBusy(true);
    setError("");
    try {
      const receipt = await api<QoinChatMessageIndexReceipt>(
        `/api/v1/workspaces/${selected.id}/chats/${activeChat}/messages/${message.id}/qoin-index`,
        {
          method: "POST",
          body: JSON.stringify({
            symbol: qoinSymbol.trim(),
            tags: [message.role, "source:chat_message", `symbol:${qoinSymbol.trim().toUpperCase()}`],
            dependency_ids: [],
            evidence_score: 0.5
          })
        }
      );
      setQoinNotice(`Indexed chat message ${receipt.message_id.slice(0, 8)} using ${receipt.symbol} at ${new Date(receipt.qoin_event_ts_ms).toLocaleString()} · available after the next hourly epoch.`);
      setError("");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not index this chat message with Qoin.");
    } finally { setBusy(false); }
  }

  async function indexMemoryWithQoin(entry: ProjectMemoryEntry) {
    if (!selected || !activeChat || !qoinSymbol.trim()) return;
    setBusy(true);
    setError("");
    try {
      const receipt = await api<QoinMemoryIndexReceipt>(
        `/api/v1/workspaces/${selected.id}/chats/${activeChat}/memory-index/${entry.id}/qoin`,
        {
          method: "POST",
          body: JSON.stringify({
            symbol: qoinSymbol.trim(),
            visibility: "workspace_shared",
            tags: [entry.type, `symbol:${qoinSymbol.trim().toUpperCase()}`],
            dependency_ids: [],
            outcome_score: 0.5,
            evidence_score: 0.5
          })
        }
      );
      setQoinNotice(`Indexed ${receipt.memory_id.slice(0, 8)} using ${receipt.symbol} at ${new Date(receipt.qoin_event_ts_ms).toLocaleString()} · available after the next hourly epoch.`);
      setError("");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not index this memory with Qoin.");
    } finally { setBusy(false); }
  }

  async function prepareAgentHandover() {
    if (!selected || !handoverAgentId || !qoinVector) return;
    setBusy(true);
    setError("");
    try {
      const result = await api<HandoverContext>(
        `/api/v1/workspaces/${selected.id}/agents/${handoverAgentId}/memory-context`,
        {
          method: "POST",
          body: JSON.stringify({
            workspace_id: selected.id,
            agent_id: handoverAgentId,
            task_id: handoverTaskId || null,
            project_id: null,
            query: qoinVector.vector,
            heads: 2,
            tags: [`symbol:${qoinVector.symbol}`],
            dependency_ids: [],
            pareto: { initial_share: 0.2, target_contribution_coverage: 0.8 }
          })
        }
      );
      setHandover(result);
      setProgressiveContext(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not prepare agent memory context.");
    } finally { setBusy(false); }
  }

  async function retrieveMoreContext() {
    if (!selected || !handoverAgentId || !handover || !qoinVector) return;
    setBusy(true);
    setError("");
    try {
      const result = await api<ProgressiveMemoryContext>(
        `/api/v1/workspaces/${selected.id}/agents/${handoverAgentId}/memory-sessions/${handover.session_id}/retrieve`,
        {
          method: "POST",
          body: JSON.stringify({
            query: qoinVector.vector,
            heads: 2,
            tags: [`symbol:${qoinVector.symbol}`],
            dependency_ids: [],
            limit: 10
          })
        }
      );
      setProgressiveContext(result);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not retrieve additional context.");
    } finally { setBusy(false); }
  }

  async function runForm(event: FormEvent<HTMLFormElement>, action: () => Promise<void>) {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The request failed.");
    } finally {
      setBusy(false);
    }
  }

  async function createWorkspace(event: FormEvent<HTMLFormElement>) {
    await runForm(event, async () => {
      const created = await api<Workspace>("/api/v1/workspaces", {
        method: "POST",
        body: JSON.stringify({ name: name.trim() })
      });
      setName("");
      await refreshWorkspaces();
      setSelected(created);
    });
  }

  async function createAgent(event: FormEvent<HTMLFormElement>) {
    if (!selected) return;
    await runForm(event, async () => {
      await api<Agent>(`/api/v1/workspaces/${selected.id}/agents`, {
        method: "POST",
        body: JSON.stringify({ display_name: agentName.trim(), provider: provider.trim() })
      });
      setAgentName("");
      await refreshWorkspace(selected);
    });
  }

  async function createArtifact(event: FormEvent<HTMLFormElement>) {
    if (!selected) return;
    await runForm(event, async () => {
      await api<Artifact>(`/api/v1/workspaces/${selected.id}/artifacts`, {
        method: "POST",
        body: JSON.stringify({ title: artifactTitle.trim(), content: artifactContent.trim(), source_agent_id: null })
      });
      setArtifactTitle("");
      setArtifactContent("");
      await refreshWorkspace(selected);
    });
  }

  async function createTask(event: FormEvent<HTMLFormElement>) {
    if (!selected || !assignee) return;
    await runForm(event, async () => {
      await api<Task>(`/api/v1/workspaces/${selected.id}/tasks`, {
        method: "POST",
        body: JSON.stringify({
          title: taskTitle.trim(),
          objective: taskObjective.trim(),
          assignee_agent_id: assignee,
          context_artifact_ids: selectedArtifactIds
        })
      });
      setTaskTitle("");
      setTaskObjective("");
      setSelectedArtifactIds([]);
      await refreshWorkspace(selected);
    });
  }

  async function advanceTask(task: Task, status: string) {
    if (!selected) return;
    setBusy(true);
    setError("");
    try {
      await api<Task>(`/api/v1/tasks/${task.id}/transitions`, {
        method: "POST",
        body: JSON.stringify({
          expected_version: task.version,
          status,
          note: `Moved to ${status.replaceAll("_", " ")} from the workspace interface.`
        })
      });
      await refreshWorkspace(selected);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Task update failed.");
    } finally {
      setBusy(false);
    }
  }

  async function submitTaskResponse(task: Task) {
    if (!selected) return;
    const content = responseDrafts[task.id]?.trim() ?? "";
    if (!content || busy) return;
    setBusy(true);
    setError("");
    try {
      await api(`/api/v1/tasks/${task.id}/submit`, {
        method: "POST",
        body: JSON.stringify({
          expected_version: task.version,
          title: `${task.title} — response`,
          content
        })
      });
      setResponseDrafts(current => ({ ...current, [task.id]: "" }));
      await refreshWorkspace(selected);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Response submission failed.");
    } finally {
      setBusy(false);
    }
  }

  const nextStatus: Record<string, string> = {
    offered: "acknowledged",
    acknowledged: "in_progress",
    in_progress: "submitted",
    submitted: "review_required",
    review_required: "accepted",
    changes_requested: "in_progress",
    accepted: "completed"
  };

  const currentUser = auth?.currentUser;
  const profileName = currentUser?.displayName || currentUser?.email?.split("@")[0] || "Your profile";
  const avatarLetter = profileName.slice(0, 1).toUpperCase();

  return (
    <main className="axon-app">
      <header className="axon-app-header">
        <div className="axon-brand"><span className="axon-brand-mark">A</span><span>AXON</span></div>
        <div className="axon-header-actions">
          <span className={health?.status === "ok" ? "axon-connection online" : "axon-connection"}><i />{health?.status === "ok" ? "Connected" : "Offline"}</span>
          <button type="button" className="axon-icon-button" aria-label="Open app menu" onClick={() => setMenuOpen(value => !value)}>☰</button>
          <button type="button" className="axon-profile-button" aria-label="Profile menu" onClick={() => setMenuOpen(value => !value)}>{currentUser?.photoURL ? <img src={currentUser.photoURL} alt="" /> : avatarLetter}</button>
        </div>
        {menuOpen && <div className="axon-menu">
          <strong>{profileName}</strong><small>{currentUser?.email ?? "Personal workspace"}</small>
          <button type="button" onClick={() => { setActiveTab("explore"); setMenuOpen(false); }}>Explore workspace</button>
          <button type="button" onClick={() => { setSelected(null); setMenuOpen(false); }}>Switch workspace</button>
          <button type="button" onClick={() => { if (auth) void import("firebase/auth").then(({signOut}) => signOut(auth)); setMenuOpen(false); }}>Sign out</button>
        </div>}
      </header>

      <nav className="axon-mobile-tabs" aria-label="Main navigation">
        <button type="button" className={activeTab === "chats" ? "active" : ""} onClick={() => setActiveTab("chats")}><span>◉</span>Chats</button>
        <button type="button" className={activeTab === "explore" ? "active" : ""} onClick={() => setActiveTab("explore")}><span>⌕</span>Explore</button>
      </nav>

      <section className="axon-desktop-layout">
        <aside className="axon-profile-pane">
          <div className="axon-profile-card">
            <div className="axon-avatar">{currentUser?.photoURL ? <img src={currentUser.photoURL} alt="" /> : avatarLetter}</div>
            <strong>{profileName}</strong><small>{currentUser?.email ?? "Axon member"}</small>
            <span className="axon-presence"><i /> Available</span>
          </div>
          <div className="axon-pane-section"><span className="axon-section-label">WORKSPACE</span>
            {selected ? <button type="button" className="axon-workspace-choice active" onClick={() => setActiveTab("explore")}><span className="axon-workspace-glyph">{selected.name.slice(0,1).toUpperCase()}</span><span><strong>{selected.name}</strong><small>Personal workspace</small></span></button> : <p className="muted">Choose a workspace in Explore.</p>}
          </div>
          <div className="axon-profile-footer"><span className={health?.status === "ok" ? "axon-health-dot online" : "axon-health-dot"} />{health?.status === "ok" ? "Axon is connected" : "Connecting to Axon"}</div>
        </aside>

        <aside className={"axon-chats-pane" + (activeTab === "explore" ? " mobile-hidden" : "")}>
          <div className="axon-pane-heading"><div><span className="axon-section-label">YOUR CONVERSATIONS</span><strong>Chats</strong></div><span className="axon-chat-count">{chats.length}</span></div>
          <form className="axon-new-chat" onSubmit={createChat}><input aria-label="New chat title" value={chatTitle} onChange={event => setChatTitle(event.target.value)} placeholder="Start a new chat…" maxLength={160} required /><button disabled={!chatTitle.trim() || busy} aria-label="Create chat">＋</button></form>
          <div className="axon-chat-list">
            {chats.map(chat => <button type="button" key={chat.id} className={activeChat === chat.id ? "axon-chat-item active" : "axon-chat-item"} onClick={() => { setActiveChat(chat.id); setActiveTab("chats"); }}>
              <span className="axon-chat-avatar">{chat.title.slice(0,1).toUpperCase()}</span><span className="axon-chat-copy"><strong>{chat.title}</strong><small>{new Date(chat.updated_at).toLocaleDateString()}</small></span><span className="axon-chat-chevron">›</span>
            </button>)}
            {chats.length === 0 && <div className="axon-empty-list"><span>◌</span><strong>No conversations yet</strong><small>Create a chat to begin working with your agents.</small></div>}
          </div>
        </aside>

        <section className={"axon-conversation-pane" + (activeTab === "explore" ? " mobile-hidden" : "")}>
          {!selected ? <div className="axon-empty-conversation"><span>✳</span><strong>Your workspace is ready when you are.</strong><p>Create or select a workspace from Explore to start a conversation.</p><button type="button" onClick={() => setActiveTab("explore")}>Explore workspaces</button></div> : !chatDetail ? <div className="axon-empty-conversation"><span>✳</span><strong>Start a conversation</strong><p>Select a chat or create a new one. Your most recent conversation will open here.</p></div> : <>
            <div className="axon-conversation-header"><div className="axon-conversation-avatar">{chatDetail.chat.title.slice(0,1).toUpperCase()}</div><div><strong>{chatDetail.chat.title}</strong><small>{selected.name} · private workspace chat</small></div><button type="button" className="axon-more-button" onClick={() => void archiveChat()} disabled={busy} title="Archive conversation">•••</button></div>
            <div className="axon-resume-brief"><span>CONTEXT BRIEF</span><p>{chatDetail.resume_brief || "Useful context approved in this conversation will appear here for future handovers."}</p></div>
            <div className="axon-message-stream">
              {chatDetail.messages.map(message => <article key={message.id} className={"axon-message-row " + (message.role === "user" ? "mine" : "theirs")}>
                <div className="axon-message-avatar">{message.role === "user" ? avatarLetter : (message.answered_by || message.role).slice(0,1).toUpperCase()}</div>
                <div className="axon-message-content"><div className="axon-message-meta"><strong>{message.role === "user" ? "You" : message.answered_by || message.role}</strong>{message.answered_by && message.role !== "user" ? <span>Agent response</span> : null}</div><div className="axon-message-bubble"><p>{message.content}</p></div><button type="button" className="axon-message-action" disabled={busy} onClick={() => void indexChatMessageWithQoin(message)}>Index with Qoin</button></div>
              </article>)}
              {chatDetail.messages.length === 0 && <div className="axon-empty-stream"><span>✧</span><strong>This conversation is new</strong><p>Send a message to start the working history.</p></div>}
            </div>
            <form className="axon-composer" onSubmit={sendChatMessage}><textarea value={messageDraft} onChange={event => setMessageDraft(event.target.value)} rows={2} maxLength={256000} placeholder="Message your workspace…" required /><div className="axon-composer-footer"><span>Messages become part of this workspace's history.</span><button disabled={!messageDraft.trim() || busy}>{busy ? "Sending…" : "Send"} <span>↗</span></button></div></form>
            <details className="axon-memory-drawer"><summary>Project memory <span>{chatDetail.memory.length} entries</span></summary>
              <form className="axon-memory-form" onSubmit={proposeMemory}><select value={memoryType} onChange={event => setMemoryType(event.target.value as ProjectMemoryEntry["type"])}><option value="note">Note</option><option value="decision">Decision</option><option value="fact">Fact</option><option value="constraint">Constraint</option><option value="open_task">Open task</option></select><input value={memoryDraft} onChange={event => setMemoryDraft(event.target.value)} placeholder="Propose a useful memory…" maxLength={4000} required /><button disabled={!memoryDraft.trim() || busy}>Add</button></form>
              {chatDetail.memory.map(entry => <div className="axon-memory-entry" key={entry.id}><div><strong>{entry.type.replaceAll("_"," ")}</strong><small>{entry.status}</small></div><p>{entry.text}</p>{entry.status === "proposed" && <div className="axon-memory-actions"><button type="button" onClick={() => void reviewMemory(entry,"approve")} disabled={busy}>Approve</button><button type="button" onClick={() => void reviewMemory(entry,"reject")} disabled={busy}>Reject</button></div>}</div>)}
            </details>
          </>}
        </section>

        <section className={"axon-explore-pane" + (activeTab === "explore" ? " active" : "")}>
          <div className="axon-explore-heading"><span className="axon-section-label">DISCOVER YOUR WORKSPACE</span><strong>Explore</strong><p>Manage agents, context and handoffs across your projects.</p></div>
          <div className="axon-workspace-switcher">
            <span className="axon-section-label">WORKSPACES</span>
            <form onSubmit={createWorkspace}><input id="workspace-name" value={name} onChange={event => setName(event.target.value)} placeholder="New workspace name" maxLength={120} required /><button disabled={!name.trim() || busy}>＋</button></form>
            <div className="axon-workspace-list">{workspaces.map(workspace => <button type="button" key={workspace.id} className={selected?.id === workspace.id ? "active" : ""} onClick={() => { setSelected(workspace); setActiveTab("explore"); }}><span className="axon-workspace-glyph">{workspace.name.slice(0,1).toUpperCase()}</span><span><strong>{workspace.name}</strong><small>{new Date(workspace.created_at).toLocaleDateString()}</small></span><span>›</span></button>)}</div>
          </div>
          {selected ? <div className="axon-explore-tools">{explore}</div> : <div className="axon-empty-explore">Select a workspace to manage its agents, artifacts, Qoin context and task handoffs.</div>}
        </section>
      </section>
      {error && <div className="axon-error" role="alert">{error}<button type="button" onClick={() => selected ? void refreshWorkspace(selected) : void refreshWorkspaces()}>Retry</button></div>}
    </main>
  );
}


function AuthGate() {
  const [user, setUser] = useState<import("firebase/auth").User | null>(null);
  const [ready, setReady] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [mode, setMode] = useState<"sign_in" | "create">("sign_in");
  const [authError, setAuthError] = useState("");
  const [working, setWorking] = useState(false);

  useEffect(() => {
    if (!auth) { setReady(true); return; }
    return onAuthStateChanged(auth, current => { setUser(current); setReady(true); });
  }, []);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!auth) return;
    setWorking(true);
    setAuthError("");
    try {
      if (mode === "create") await createUserWithEmailAndPassword(auth, email.trim(), password);
      else await signInWithEmailAndPassword(auth, email.trim(), password);
    } catch (cause) {
      setAuthError(cause instanceof Error ? cause.message : "Authentication failed.");
    } finally { setWorking(false); }
  }

  if (!ready) return <main className="shell"><section className="content"><p>Restoring your secure session…</p></section></main>;
  if (!auth) return <main className="shell"><section className="content"><h1>Firebase configuration required</h1><p>Set the VITE_FIREBASE_* environment variables for this web app, then restart it.</p></section></main>;
  if (user) return <><div style={{position:"fixed",right:24,top:18,zIndex:10,display:"flex",gap:12,alignItems:"center",fontSize:12}}><span>{user.email}</span><button onClick={() => { if (auth) void signOut(auth); }}>Sign out</button></div><WorkspaceApp /></>;
  return <main className="shell"><section className="content" style={{maxWidth:520,margin:"12vh auto"}}><p className="eyebrow">AXON · SECURE ACCESS</p><h1>{mode === "create" ? "Create your account" : "Welcome back"}</h1><p>Sign in to access your personal workspace and its data.</p><form className="stack-form" onSubmit={submit}><label htmlFor="auth-email">Email</label><input id="auth-email" type="email" autoComplete="email" required value={email} onChange={e=>setEmail(e.target.value)}/><label htmlFor="auth-password">Password</label><input id="auth-password" type="password" minLength={6} autoComplete={mode === "create" ? "new-password" : "current-password"} required value={password} onChange={e=>setPassword(e.target.value)}/><button disabled={working}>{working ? "Please wait…" : mode === "create" ? "Create account" : "Sign in"}</button></form>{authError && <p role="alert">{authError}</p>}<button className="back-link" onClick={()=>setMode(mode === "create" ? "sign_in" : "create")}>{mode === "create" ? "Already registered? Sign in" : "New to Axon? Create an account"}</button></section></main>;
}

import { onAuthStateChanged, signInWithEmailAndPassword, createUserWithEmailAndPassword, signOut } from "firebase/auth";

export default function App() { return <AuthGate />; }
