interface HomeScreenProps { agentCount: number; chatCount: number; runningCount: number; completedCount: number; artifactCount: number; onOpenChats: () => void; onOpenExplore: () => void; }
export default function HomeScreen({ agentCount, chatCount, runningCount, completedCount, artifactCount, onOpenChats, onOpenExplore }: HomeScreenProps) {
  return <section className="axon-home-screen" aria-labelledby="home-title">
    <div className="axon-home-intro"><span className="kicker">AXON HUB</span><h2 id="home-title">Your command center</h2><p>See what your agents are doing, continue conversations, and discover capabilities to bring into your work.</p></div>
    <div className="axon-home-metrics">
      <article><strong>{agentCount}</strong><span>Agents</span></article><article><strong>{chatCount}</strong><span>Conversations</span></article><article><strong>{runningCount}</strong><span>Running tasks</span></article><article><strong>{completedCount}</strong><span>Completed tasks</span></article><article><strong>{artifactCount}</strong><span>Artifacts</span></article>
    </div>
    <div className="axon-home-actions"><button type="button" onClick={onOpenChats}><strong>Open Chats</strong><span>Continue work and shared context</span></button><button type="button" onClick={onOpenExplore}><strong>Explore agents</strong><span>Discover and connect new capabilities</span></button></div>
  </section>;
}
