import { useEffect, useState } from "react";
import { auth } from "./firebase";

type Provider = {
  id: string; name: string; base_url: string; status: string;
  data_terms: { processing_regions: string[]; trains_on_api_inputs: boolean | null; retention_days: number | null; retention_description: string };
  terms_reviewed_at: string | null;
};
type Model = {
  id: string; provider_id: string; model_name: string; status: "candidate" | "approved" | "disabled";
  abilities: string[]; categories: string[]; context_length: number;
  input_cost_per_million: number; output_cost_per_million: number;
  typical_latency_ms: number; version: string | null; healthy: boolean; flagged_for_review: boolean;
};
type Evaluation = { model_id: string; category: string; score: number; test_set_version: string; run_at: string; cost_micros: number };
type Stat = { model_id: string; category: string; role: string; runs: number; completion_rate: number; supervisor_pass_rate: number; user_retry_rate: number; cost_per_run_micros: number; latency_ms: number };
type Flag = { id: string; model_id: string; change_kind: string; previous_value: string | null; observed_value: string | null; detected_at: string; cleared_at: string | null; resolution_note: string | null };
type Decision = { run_id: string; role: string; slot: number; candidates_considered: string[]; chosen_model_id: string; reason: string; estimated_cost_micros: number; created_at: string };

async function adminApi<T>(path: string, init?: RequestInit): Promise<T> {
  const token = await auth?.currentUser?.getIdToken();
  const response = await fetch(path, {
    ...init,
    headers: { "Content-Type": "application/json", ...(token ? { Authorization: `Bearer ${token}` } : {}), ...init?.headers }
  });
  if (!response.ok) {
    const payload = await response.json().catch(() => ({ error: response.statusText }));
    throw new Error(payload.error ?? "Admin request failed");
  }
  return response.json() as Promise<T>;
}

export function ModelAdmin({ onClose }: { onClose: () => void }) {
  const [providers, setProviders] = useState<Provider[]>([]);
  const [models, setModels] = useState<Model[]>([]);
  const [evaluations, setEvaluations] = useState<Evaluation[]>([]);
  const [stats, setStats] = useState<Stat[]>([]);
  const [decisions, setDecisions] = useState<Decision[]>([]);
  const [flags, setFlags] = useState<Flag[]>([]);
  const [flagNotes, setFlagNotes] = useState<Record<string, string>>({});
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function refresh() {
    const [providerRows, modelRows, evaluationRows, statRows, decisionRows, flagRows] = await Promise.all([
      adminApi<Provider[]>("/api/v1/admin/providers"),
      adminApi<Model[]>("/api/v1/admin/models"),
      adminApi<Evaluation[]>("/api/v1/admin/evaluations"),
      adminApi<Stat[]>("/api/v1/admin/stats"),
      adminApi<Decision[]>("/api/v1/admin/assignments"),
      adminApi<Flag[]>("/api/v1/admin/flags")
    ]);
    setProviders(providerRows);
    setModels(modelRows);
    setEvaluations(evaluationRows);
    setStats(statRows);
    setDecisions(decisionRows);
    setFlags(flagRows);
  }

  useEffect(() => {
    void refresh().catch(cause => setError(cause instanceof Error ? cause.message : "Could not load model registry."));
  }, []);

  async function review(model: Model, action: "approve" | "reject") {
    const note = notes[model.id]?.trim();
    if (!note || busy) return;
    setBusy(true);
    setError("");
    try {
      await adminApi<Model>(`/api/v1/admin/models/${model.id}/review`, {
        method: "POST", body: JSON.stringify({ action, note })
      });
      setNotes(current => ({ ...current, [model.id]: "" }));
      await refresh();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Candidate review failed.");
    } finally {
      setBusy(false);
    }
  }

  async function clearFlag(flag: Flag) {
    const note = flagNotes[flag.id]?.trim();
    if (!note || busy) return;
    setBusy(true);
    setError("");
    try {
      await adminApi<Flag>(`/api/v1/admin/flags/${flag.id}/clear`, { method: "POST", body: JSON.stringify({ note }) });
      setFlagNotes(current => ({ ...current, [flag.id]: "" }));
      await refresh();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not resolve model flag.");
    } finally { setBusy(false); }
  }

  const providerName = (id: string) => providers.find(provider => provider.id === id)?.name ?? "Unknown provider";
  const latestEvaluation = (modelId: string) => evaluations.filter(item => item.model_id === modelId)
    .sort((a, b) => b.run_at.localeCompare(a.run_at))[0];
  const modelStats = (modelId: string) => stats.filter(item => item.model_id === modelId);

  return (
    <section className="model-admin">
      <header className="model-admin-header">
        <div><span className="kicker">PLATFORM ADMINISTRATION</span><h2>Model registry</h2><p>Provider terms, candidate reviews and assignment evidence.</p></div>
        <button type="button" className="secondary" onClick={onClose}>Back to workspace</button>
      </header>

      <section className="panel">
        <div className="panel-heading"><div><span className="kicker">01 · PROVIDERS</span><h3>Provider data terms</h3></div><span className="count">{providers.length} PROVIDERS</span></div>
        <div className="admin-table">
          {providers.map(provider => <article className="admin-row" key={provider.id}>
            <div><strong>{provider.name}</strong><small>{provider.base_url}</small></div>
            <div><span className={provider.terms_reviewed_at ? "admin-status approved" : "admin-status candidate"}>{provider.terms_reviewed_at ? "Terms reviewed" : "Terms not reviewed"}</span><small>Regions: {provider.data_terms.processing_regions.join(", ") || "Not recorded"}</small></div>
            <div><small>Training on API inputs: {provider.data_terms.trains_on_api_inputs === null ? "Unknown" : provider.data_terms.trains_on_api_inputs ? "Yes" : "No"}</small><small>Retention: {provider.data_terms.retention_days === null ? "Unknown" : `${provider.data_terms.retention_days} days`}</small></div>
          </article>)}
          {providers.length === 0 && <p className="muted">No providers configured. Provider registration and discovery sources must be configured by an administrator.</p>}
        </div>
      </section>

      <section className="panel">
        <div className="panel-heading"><div><span className="kicker">02 · MODEL CATALOG</span><h3>Models and candidate review</h3></div><span className="count">{models.length} MODELS</span></div>
        <div className="admin-model-list">
          {models.map(model => {
            const evaluation = latestEvaluation(model.id);
            const rows = modelStats(model.id);
            return <article className="admin-model" key={model.id}>
              <div className="admin-model-head"><div><strong>{model.model_name}</strong><small>{providerName(model.provider_id)} · {model.version ?? "version unknown"}</small></div><span className={`admin-status ${model.status}`}>{model.status}</span></div>
              <div className="admin-model-meta"><span>Abilities: {model.abilities.join(", ")}</span><span>Categories: {model.categories.join(", ")}</span><span>Context: {model.context_length.toLocaleString()}</span></div>
              <div className="admin-model-meta"><span>Input / 1M: ${model.input_cost_per_million}</span><span>Output / 1M: ${model.output_cost_per_million}</span><span>Typical latency: {model.typical_latency_ms} ms</span></div>
              <div className="admin-model-meta"><span>Latest score: {evaluation ? `${(evaluation.score * 100).toFixed(1)}% · ${evaluation.category} · ${evaluation.test_set_version}` : "Not evaluated"}</span><span>Healthy: {model.healthy ? "Yes" : "No"}</span><span>Flagged: {model.flagged_for_review ? "Yes" : "No"}</span></div>
              {rows.length > 0 && <div className="admin-model-meta">{rows.map(row => <span key={row.role + row.category}>{row.category}/{row.role}: {row.runs} runs · {(row.supervisor_pass_rate * 100).toFixed(0)}% pass · {row.latency_ms} ms</span>)}</div>}
              {model.status === "candidate" && <div className="candidate-review"><label htmlFor={`review-${model.id}`}>Required review note</label><textarea id={`review-${model.id}`} rows={2} value={notes[model.id] ?? ""} onChange={event => setNotes(current => ({ ...current, [model.id]: event.target.value }))} placeholder="Record what you checked about the model and provider terms…" /><div><button type="button" disabled={busy || !notes[model.id]?.trim()} onClick={() => void review(model, "approve")}>Approve candidate</button><button type="button" className="secondary" disabled={busy || !notes[model.id]?.trim()} onClick={() => void review(model, "reject")}>Reject candidate</button></div></div>}
            </article>;
          })}
          {models.length === 0 && <p className="muted">No models registered. Newly discovered models must remain candidates until evaluated and explicitly approved.</p>}
        </div>
      </section>

      <section className="panel">
        <div className="panel-heading"><div><span className="kicker">03 · CHANGE CONTROL</span><h3>Flagged model changes</h3></div><span className="count">{flags.filter(flag => !flag.cleared_at).length} OPEN FLAGS</span></div>
        <div className="admin-model-list">
          {flags.map(flag => <article className="admin-model" key={flag.id}>
            <div className="admin-model-head"><div><strong>{models.find(model => model.id === flag.model_id)?.model_name ?? flag.model_id}</strong><small>{flag.change_kind} · {new Date(flag.detected_at).toLocaleString()}</small></div><span className={flag.cleared_at ? "admin-status approved" : "admin-status candidate"}>{flag.cleared_at ? "Resolved" : "Review required"}</span></div>
            <div className="admin-model-meta"><span>Previous: {flag.previous_value ?? "Unknown"}</span><span>Observed: {flag.observed_value ?? "Unknown"}</span></div>
            {flag.cleared_at ? <p className="muted">Resolution: {flag.resolution_note}</p> : <div className="candidate-review"><label htmlFor={`flag-${flag.id}`}>Resolution note</label><textarea id={`flag-${flag.id}`} rows={2} value={flagNotes[flag.id] ?? ""} onChange={event => setFlagNotes(current => ({ ...current, [flag.id]: event.target.value }))} placeholder="Explain the review and why this model can be re-enabled…" /><div><button type="button" disabled={busy || !flagNotes[flag.id]?.trim()} onClick={() => void clearFlag(flag)}>Clear flag</button></div></div>}
          </article>)}
          {flags.length === 0 && <p className="muted">No provider, price or evaluation changes have been flagged.</p>}
        </div>
      </section>

      <section className="panel">
        <div className="panel-heading"><div><span className="kicker">04 · ASSIGNMENT AUDIT</span><h3>Recent decisions</h3></div><span className="count">{decisions.length} DECISIONS</span></div>
        <div className="admin-table">
          {decisions.slice().sort((a, b) => b.created_at.localeCompare(a.created_at)).slice(0, 50).map(decision => <article className="admin-row" key={decision.run_id + decision.role + decision.slot}>
            <div><strong>{decision.role} · slot {decision.slot + 1}</strong><small>Run {decision.run_id}</small></div>
            <div><strong>{models.find(model => model.id === decision.chosen_model_id)?.model_name ?? decision.chosen_model_id}</strong><small>{providerName(models.find(model => model.id === decision.chosen_model_id)?.provider_id ?? "")}</small></div>
            <div><small>{decision.reason}</small><small>{decision.candidates_considered.length} candidates · ${decision.estimated_cost_micros / 1_000_000} estimated</small></div>
          </article>)}
          {decisions.length === 0 && <p className="muted">No assignment decisions have been recorded yet.</p>}
        </div>
      </section>

      {error && <p className="error" role="alert">{error}</p>}
      <button type="button" className="secondary" disabled={busy} onClick={() => void refresh()}>Refresh registry</button>
    </section>
  );
}
