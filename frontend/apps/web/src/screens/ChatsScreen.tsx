import type { ReactNode } from "react";
interface ChatsScreenProps { children: ReactNode; }
export default function ChatsScreen({ children }: ChatsScreenProps) {
  return <section className="axon-destination axon-chats-screen" aria-label="Chats screen">{children}</section>;
}
