import type { AiModel, AiSettings } from "./api";

/** Ready-made services. Several have a free tier; the model lists come from the service itself, so they are never out of date. */
export const AI_PRESETS: { id: string; label: string; endpoint: string; key: boolean; keyUrl: string | null; note: string }[] = [
  { id: "ollama", label: "Ollama (on this computer)", endpoint: "http://localhost:11434/v1", key: false, keyUrl: "https://ollama.com/download", note: "Free and private: nothing leaves your computer. Install Ollama, run it, and pull a model (for example: ollama pull qwen2.5-coder:7b)." },
  { id: "openrouter", label: "OpenRouter (free models)", endpoint: "https://openrouter.ai/api/v1", key: true, keyUrl: "https://openrouter.ai/keys", note: "Many models marked free. Make a free key, paste it, and press Connect." },
  { id: "groq", label: "Groq (free tier)", endpoint: "https://api.groq.com/openai/v1", key: true, keyUrl: "https://console.groq.com/keys", note: "Fast, with a free tier. Make a key, paste it, and press Connect." },
  { id: "gemini", label: "Google Gemini (free tier)", endpoint: "https://generativelanguage.googleapis.com/v1beta/openai", key: true, keyUrl: "https://aistudio.google.com/apikey", note: "Free tier with a Google account. Make a key, paste it, and press Connect." },
  { id: "anthropic", label: "Claude (API key, paid)", endpoint: "https://api.anthropic.com", key: true, keyUrl: "https://console.anthropic.com/settings/keys", note: "Uses Claude's API, which is billed separately from a Claude subscription. Sign in on the Anthropic console, create a key, paste it, and press Connect." },
  { id: "custom", label: "Other (OpenAI-compatible)", endpoint: "", key: true, keyUrl: null, note: "Any service with an OpenAI-style address (ending in /v1)." },
];

const KEY = "mylodb.ai";
export interface AiConfig { preset: string; endpoint: string; model: string; context: number | null; auto: boolean; models: AiModel[] }

export function loadAi(): AiConfig | null {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "null");
    if (v && typeof v.endpoint === "string" && typeof v.model === "string" && v.endpoint && v.model) return { context: null, auto: true, models: [], ...v };
  } catch { /* a damaged entry counts as not set up */ }
  return null;
}
export function saveAi(c: AiConfig | null) { if (c) localStorage.setItem(KEY, JSON.stringify(c)); else localStorage.removeItem(KEY); }

/** What goes to the backend: the chosen model plus, when switching is on, the other models of the list (largest window first). */
export function toSettings(c: AiConfig): AiSettings {
  const chosen = c.models.find((m) => m.id === c.model);
  const fallbacks = c.auto ? c.models.filter((m) => m.id !== c.model).slice(0, 6).map((m) => ({ id: m.id, context: m.context })) : [];
  return { preset: c.preset, endpoint: c.endpoint, model: c.model, fallbacks, context: chosen?.context ?? c.context };
}

/** The user confirms once per service that the table and column names of a database go to it. */
const consentKey = (endpoint: string) => `mylodb.ai.consent.${endpoint}`;
export const hasConsent = (endpoint: string) => localStorage.getItem(consentKey(endpoint)) === "1";
export const giveConsent = (endpoint: string) => localStorage.setItem(consentKey(endpoint), "1");
export const isLocal = (endpoint: string) => /^http:\/\/(localhost|127\.0\.0\.1|\[::1\])([:/]|$)/.test(endpoint);

export const contextLabel = (n: number | null) => (n ? (n >= 1000 ? `${Math.round(n / 1000)}k` : String(n)) : "");
