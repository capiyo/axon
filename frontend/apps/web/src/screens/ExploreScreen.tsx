import type { ReactNode } from "react";
interface ExploreScreenProps { children: ReactNode; }
export default function ExploreScreen({ children }: ExploreScreenProps) {
  return <section className="axon-destination axon-explore-screen" aria-label="Explore screen">{children}</section>;
}
