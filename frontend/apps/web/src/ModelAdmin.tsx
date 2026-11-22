import { useEffect, useState } from "react";
import type { FormEvent } from "react";
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
  const [providerNameDraft, setProviderNameDraft] = useState("");
  const [baseUrlDraft, setBaseUrlDraft] = useState("");
  const [discoveryUrlDraft, setDiscoveryUrlDraft] = useState("");
  const [pricingUrlDraft, setPricingUrlDraft] = useState("");
  const [apiKeyEnvDraft, setApiKeyEnvDraft] = useState("");
  const [termsProviderId, setTermsProviderId] = useState("");
  const [termsRegions, setTermsRegions] = useState("us");
  const [termsTraining, setTermsTraining] = useState("unknown");
  const [termsRetention, setTermsRetention] = useState("unknown");
  const [termsDescription, setTermsDescription] = useState("");
  const [modelProviderId, setModelProviderId] = useState("");
  const [modelNameDraft, setModelNameDraft] = useState("");
  const [modelAbilities, setModelAbilities] = useState("text");
  const [modelCategories, setModelCategories] = useState("general");
  const [modelRegions, setModelRegions] = useState("us");
  const [modelContext, setModelContext] = useState("32000");
  const [modelInputCost, setModelInputCost] = useState("0");
  const [modelOutputCost, setModelOutputCost] = useState("0");
  const [modelLatency, setModelLatency] = useState("1000");
  const [modelVersion, setModelVersion] = useState("");
  const [modelNotes, setModelNotes] = useState("");

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

  async function createProvider(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy) return;
    setBusy(true); setError("");
    try {
      await adminApi<Provider>("/api/v1/admin/providers", {
        method: "POST",
        body: JSON.stringify({
          name: providerNameDraft.trim(), base_url: baseUrlDraft.trim(),
          model_discovery_url: discoveryUrlDraft.trim() || null,
          pricing_source_url: pricingUrlDraft.trim() || null,
          api_key_env: apiKeyEnvDraft.trim() || null,
          discovery_enabled: Boolean(discoveryUrlDraft.trim()), status: "active"
        })
      });
      setProviderNameDraft(""); setBaseUrlDraft(""); setDiscoveryUrlDraft(""); setPricingUrlDraft(""); setApiKeyEnvDraft("");
      await refresh();
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Provider registration failed."); }
    finally { setBusy(false); }
  }

  async function saveProviderTerms(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy || !termsProviderId) return;
    setBusy(true); setError("");
    try {
      await adminApi<Provider>(`/api/v1/admin/providers/${termsProviderId}/terms-review`, {
        method: "POST",
        body: JSON.stringify({
          processing_regions: termsRegions.split(",").map(value => value.trim()).filter(Boolean),
          trains_on_api_inputs: termsTraining === "unknown" ? null : termsTraining === "yes",
          retention_days: termsRetention === "unknown" ? null : Number(termsRetention),
          retention_description: termsDescription.trim()
        })
      });
      setTermsProviderId(""); setTermsDescription("");
      await refresh();
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Provider terms review failed."); }
    finally { setBusy(false); }
  }

  async function createModel(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy || !modelProviderId) return;
    setBusy(true); setError("");
    try {
      await adminApi<Model>("/api/v1/admin/models", {
        method: "POST",
        body: JSON.stringify({
          provider_id: modelProviderId, model_name: modelNameDraft.trim(),
          abilities: modelAbilities.split(",").map(value => value.trim()).filter(Boolean),
          categories: modelCategories.split(",").map(value => value.trim()).filter(Boolean),
          regions: modelRegions.split(",").map(value => value.trim()).filter(Boolean),
          context_length: Number(modelContext), input_cost_per_million: Number(modelInputCost),
          output_cost_per_million: Number(modelOutputCost), typical_latency_ms: Number(modelLatency),
          version: modelVersion.trim() || null, notes: modelNotes.trim()
        })
      });
      setModelNameDraft(""); setModelVersion(""); setModelNotes("");
      await refresh();
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Model registration failed."); }
    finally { setBusy(false); }
  }

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
          {providers.length === 0 && <p className="muted">No providers configured. Register an explicitly configured provider source below.</p>}
        </div>
        <form className="admin-form" onSubmit={createProvider}>
          <h4>Register provider and catalog sources</h4>
          <div className="admin-form-grid">
            <label>Provider name<input value={providerNameDraft} onChange={event => setProviderNameDraft(event.target.value)} required /></label>
            <label>Base URL<input type="url" value={baseUrlDraft} onChange={event => setBaseUrlDraft(event.target.value)} placeholder="https://api.provider.com" required /></label>
            <label>Model discovery JSON URL<input type="url" value={discoveryUrlDraft} onChange={event => setDiscoveryUrlDraft(event.target.value)} placeholder="https://catalog.example/models.json" /></label>
            <label>Pricing JSON URL<input type="url" value={pricingUrlDraft} onChange={event => setPricingUrlDraft(event.target.value)} placeholder="https://catalog.example/prices.json" /></label>
            <label>API key environment variable<input value={apiKeyEnvDraft} onChange={event => setApiKeyEnvDraft(event.target.value)} placeholder="AXON_PROVIDER_KEY" /></label>
          </div>
          <button disabled={busy || !providerNameDraft.trim() || !baseUrlDraft.trim()}>Register provider</button>
        </form>
        {termsProviderId && <form className="admin-form" onSubmit={saveProviderTerms}>
          <h4>Review provider data terms: {providerName(termsProviderId)}</h4>
          <div className="admin-form-grid">
            <label>Processing regions<input value={termsRegions} onChange={event => setTermsRegions(event.target.value)} placeholder="us, eu" required /></label>
            <label>Trains on API inputs<select value={termsTraining} onChange={event => setTermsTraining(event.target.value)}><option value="unknown">Unknown</option><option value="yes">Yes</option><option value="no">No</option></select></label>
            <label>Retention days<input value={termsRetention} onChange={event => setTermsRetention(event.target.value)} placeholder="unknown or 0" /></label>
            <label>Retention description<input value={termsDescription} onChange={event => setTermsDescription(event.target.value)} required /></label>
          </div>
          <button disabled={busy || !termsDescription.trim()}>Confirm terms review</button>
          <button type="button" className="secondary" onClick={() => setTermsProviderId("")}>Cancel</button>
        </form>}
        <div className="admin-model-meta">{providers.filter(provider => !provider.terms_reviewed_at).map(provider => <button type="button" className="secondary" key={provider.id} onClick={() => setTermsProviderId(provider.id)}>Review terms for {provider.name}</button>)}</div>
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
        <form className="admin-form" onSubmit={createModel}>
          <h4>Register a candidate model manually</h4>
          <div className="admin-form-grid">
            <label>Provider<select value={modelProviderId} onChange={event => setModelProviderId(event.target.value)} required><option value="">Select provider</option>{providers.filter(provider => provider.status === "active").map(provider => <option value={provider.id} key={provider.id}>{provider.name}</option>)}</select></label>
            <label>Model name<input value={modelNameDraft} onChange={event => setModelNameDraft(event.target.value)} required /></label>
            <label>Abilities (comma separated)<input value={modelAbilities} onChange={event => setModelAbilities(event.target.value)} required /></label>
            <label>Categories (comma separated)<input value={modelCategories} onChange={event => setModelCategories(event.target.value)} required /></label>
            <label>Regions (comma separated)<input value={modelRegions} onChange={event => setModelRegions(event.target.value)} /></label>
            <label>Context length<input type="number" min="1" value={modelContext} onChange={event => setModelContext(event.target.value)} required /></label>
            <label>Input cost per million tokens<input type="number" min="0" value={modelInputCost} onChange={event => setModelInputCost(event.target.value)} required /></label>
            <label>Output cost per million tokens<input type="number" min="0" value={modelOutputCost} onChange={event => setModelOutputCost(event.target.value)} required /></label>
            <label>Typical latency ms<input type="number" min="0" value={modelLatency} onChange={event => setModelLatency(event.target.value)} required /></label>
            <label>Version<input value={modelVersion} onChange={event => setModelVersion(event.target.value)} /></label>
            <label>Notes<input value={modelNotes} onChange={event => setModelNotes(event.target.value)} /></label>
          </div>
          <button disabled={busy || !modelProviderId || !modelNameDraft.trim()}>Register as candidate</button>
        </form>
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
