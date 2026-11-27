import type { ReactNode } from "react";

interface WorkspaceViewProps { children: ReactNode; }

export default function WorkspaceView({ children }: WorkspaceViewProps) {
  return <main className="shell">{children}</main>;
}
