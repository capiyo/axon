import type { ReactNode } from "react";
interface ChatsScreenProps { children: ReactNode; }
export default function ChatsScreen({ children }: ChatsScreenProps) {
  return <section className="axon-destination axon-sessions-screen" aria-label="Sessions screen">{children}</section>;
}
