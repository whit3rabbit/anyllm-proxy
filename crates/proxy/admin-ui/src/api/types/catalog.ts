/** Details of a provider available in the LiteLLM catalog. */
export interface CatalogProvider {
  /** Unique provider identifier. */
  id: string
  /** User-facing display name. */
  display_name: string
  /** Keep for backwards compatibility. */
  name?: string
  /** Communication protocol. */
  protocol: string
  /** Auth mechanism. */
  auth: string
  /** Proxy integration status. */
  status: 'implemented' | 'wired' | 'stub'
  /** Default base URL. */
  default_base_url: string
  /** Expected environment variables for API keys. */
  env_vars: string[]
  /** LiteLLM prefix string. */
  litellm_prefix: string
  /** Supported capabilities. */
  capabilities: {
    chat_completions: boolean
    streaming: boolean
    tool_use: boolean
    embeddings: boolean
    vision: boolean
    batch: boolean
  }
  /** Total count of models. */
  model_count: number
  /** Count of models currently cached. */
  cached_model_count: number
  /** UNIX timestamp of the last cache refresh. */
  last_refreshed: number | null
}
