import type { Environment } from "./api";

export const ENV_LABEL: Record<Environment, string> = { development: "Development", staging: "Staging", production: "Production" };
/** Colour of a connection's tab and badge per environment. Development keeps the original blue. */
export const ENV_COLOR: Record<Environment, string> = { development: "#4f8cff", staging: "#e0a03b", production: "#e5484d" };
