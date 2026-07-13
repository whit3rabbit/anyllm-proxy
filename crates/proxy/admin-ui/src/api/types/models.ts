/** Represents a single model configuration entry. */
export interface ModelEntry {
  /** The unique name/identifier of the model. */
  model_name: string
  /** The number of active deployments for this model. */
  deployments: number
}

/** Response shape for models list requests. */
export interface ModelsResponse {
  /** List of model configurations. */
  models: ModelEntry[]
  /** Router strategy (e.g. priority, failover). */
  strategy: string | null
  /** Optional descriptive note. */
  note?: string
}

/** Represents a model discovered from a backend API. */
export interface DiscoveredModel {
  /** The model identifier. */
  id: string
  /** Optional human-readable name. */
  name: string | null
}

/** Response containing discovered models. */
export interface DiscoverResponse {
  /** List of discovered models. */
  models: DiscoveredModel[]
  /** Source or method used for discovery. */
  source: string
  /** True if authorization was used. */
  auth_used: boolean
}

/** Details of a model available in the LiteLLM catalog. */
export interface CatalogModel {
  /** Unique model identifier. */
  id: string
  /** Maximum context window size in tokens. */
  context_window: number
  /** Maximum output tokens. */
  max_output_tokens: number
  /** Availability status of the model. */
  status: 'available' | 'deprecated' | 'stub'
  /** Capabilities flags. */
  capabilities: {
    streaming: boolean
    tool_use: boolean
    vision: boolean
    extended_thinking: boolean
  }
  /** Pricing per million tokens. */
  pricing: {
    input_per_million_tokens: number
    output_per_million_tokens: number
  } | null
}

/** Response containing model lists. */
export interface CatalogModelsResponse {
  /** Catalog provider ID. */
  provider_id: string
  /** True if this provider has models. */
  has_models: boolean
  /** List of models. */
  models: CatalogModel[]
}
