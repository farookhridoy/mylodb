import type { AiSettings } from "./api";

/** Ready-made services. Several have a free tier; the model names change over time, so they can be edited. */
export const AI_PRESETS: { id: string; label: string; endpoint: string; model: string; key: boolean; note: string }[] = [
  { id: "ollama", label: "Ollama (on this computer)", endpoint: "http://localhost:11434/v1", model: "qwen2.5-coder:7b", key: false, note: "Free and private: nothing leaves your computer. Install Ollama from ollama.com, then run: ollama pull qwen2.5-coder:7b" },
  { id: "groq", label: "Groq (free tier)", endpoint: "https://api.groq.com/openai/v1", model: "llama-3.3-70b-versatile", key: true, note: "Fast, with a free tier. Make a key at console.groq.com." },
  { id: "openrouter", label: "OpenRouter (free models)", endpoint: "https://openrouter.ai/api/v1", model: "meta-llama/llama-3.3-70b-instruct:free", key: true, note: "Several models marked :free. Make a key at openrouter.ai." },
  { id: "gemini", label: "Google Gemini (free tier)", endpoint: "https://generativelanguage.googleapis.com/v1beta/openai", model: "gemini-2.0-flash", key: true, note: "Free tier with a Google account. Make a key at aistudio.google.com." },
  { id: "custom", label: "Other (OpenAI-compatible)", endpoint: "", model: "", key: true, note: "Any service with an OpenAI-style /chat/completions address." },
];

const KEY = "mylodb.ai";
export interface AiConfig extends AiSettings { preset: string }

export function loadAi(): AiConfig | null {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "null");
    if (v && typeof v.endpoint === "string" && typeof v.model === "string" && v.endpoint && v.model) return v;
  } catch { /* a damaged entry counts as not set up */ }
  return null;
}
export function saveAi(c: AiConfig | null) { if (c) localStorage.setItem(KEY, JSON.stringify(c)); else localStorage.removeItem(KEY); }

/** The user confirms once per service that the table and column names of a database go to it. */
const consentKey = (endpoint: string) => `mylodb.ai.consent.${endpoint}`;
export const hasConsent = (endpoint: string) => localStorage.getItem(consentKey(endpoint)) === "1";
export const giveConsent = (endpoint: string) => localStorage.setItem(consentKey(endpoint), "1");
export const isLocal = (endpoint: string) => /^http:\/\/(localhost|127\.0\.0\.1|\[::1\])([:/]|$)/.test(endpoint);
