/** Represents a single configuration entry key-value pair. */
export interface ConfigEntry {
  /** Configuration key. */
  key: string
  /** Configuration value. */
  value: string
  /** Timestamp when updated. */
  updated_at: string
}

/** Represents the full system configuration and environment variables. */
export interface ConfigResponse {
  /** List of config database overrides. */
  entries: ConfigEntry[]
  /** Server environment variables map. */
  env: Record<string, string>
  /** System logging level. */
  log_level: string
  /** Whether requests and responses are logged. */
  log_bodies: boolean
  /** Whether credentials are redacted from logs. */
  redact_secrets: boolean
  /** Whether to attempt to repair broken thinking blocks in Anthropic streams. */
  anthropic_thinking_repair: boolean
  /** True if pxpipe image compression is enabled. */
  pxpipe_compress: boolean
  /** CSV of model bases in pxpipe compression scope. */
  pxpipe_models: string
  /** Vision-capable Claude models offered as per-model scope toggles. */
  pxpipe_available_models: string[]
  /** Whether RTK tool-output compression is active. */
  rtk_compress: boolean
  /** CSV of model bases in RTK compression scope (empty = all models). */
  rtk_models: string
  /** Whether to pass client authorization headers to backend. */
  forward_client_auth: boolean
  /** Tool guardrail mode configured. */
  tool_guardrail_mode: string
  /** Prompt-compression optimizer mode: 'off' | 'shadow' | 'live'. */
  optimizer_mode: string
  /** Mappings of backend names to their configured big/small models. */
  backends: Record<string, { big_model: string; small_model: string }>
  /** List of keys whose overrides are active. */
  overridden_keys: string[]
}
