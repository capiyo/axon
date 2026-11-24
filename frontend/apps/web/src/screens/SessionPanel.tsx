import type { ReactNode } from "react";
interface SessionPanelProps { children: ReactNode; }
export default function SessionPanel({ children }: SessionPanelProps) {
  return <section className="axon-destination axon-session-panel" aria-label="Session workspace">{children}</section>;
}
