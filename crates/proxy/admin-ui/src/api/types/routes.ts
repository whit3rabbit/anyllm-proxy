/** Represents a configured API routing definition. */
export interface Route {
  /** Unique route database ID. */
  id: string
  /** Route name. */
  name: string
  /** Optional description. */
  description: string | null
  /** Load balancing strategy. */
  strategy: string
  /** Requests per minute limit. */
  rpm: number | null
  /** Tokens per minute limit. */
  tpm: number | null
  /** Cost budget in USD. */
  budget_usd: number | null
  /** Active status toggle. */
  enabled: boolean
  /** Tool guardrail mode override. */
  guardrail_mode: string | null
  /** pxpipe image compression toggle override. */
  pxpipe_compress: boolean | null
  /** pxpipe models CSV override. */
  pxpipe_models: string | null
  /** Secret redaction toggle override. */
  redact_secrets: boolean | null
  /** Position order in the router. */
  position: number
  /** Count of providers assigned to this route. */
  provider_count: number
  /** ISO 8601 creation timestamp. */
  created_at: string
  /** ISO 8601 last update timestamp. */
  updated_at: string
}

/** Response containing routing configs. */
export interface RoutesResponse {
  /** List of routes. */
  routes: Route[]
}

/** Request payload to create a new route. */
export interface CreateRouteRequest {
  /** Unique route name. */
  name: string
  /** Route description. */
  description?: string
  /** Load balancing strategy. */
  strategy?: string
  /** Requests per minute limit. */
  rpm?: number
  /** Tokens per minute limit. */
  tpm?: number
  /** Cost budget in USD. */
  budget_usd?: number
  /** Active status toggle. */
  enabled?: boolean
  /** Tool guardrail mode override. */
  guardrail_mode?: string | null
  /** pxpipe image compression toggle override. */
  pxpipe_compress?: boolean | null
  /** pxpipe models CSV override. */
  pxpipe_models?: string | null
  /** Secret redaction toggle override. */
  redact_secrets?: boolean | null
  /** Position order. */
  position?: number
}

/** Request payload to update an existing route. */
export type UpdateRouteRequest = Partial<CreateRouteRequest>

/** Represents a provider mapped to a route. */
export interface RouteProvider {
  /** Unique assignment ID. */
  id: string
  /** Target route ID. */
  route_id: string
  /** Backend ID. */
  backend_id: string
  /** Name of the backend. */
  backend_name: string
  /** Catalog provider ID. */
  provider_id: string
  /** Supported models list. */
  models: string[]
  /** Evaluation priority. */
  priority: number
  /** Active status. */
  enabled: boolean
}

/** Response containing route provider mappings. */
export interface RouteProvidersResponse {
  /** List of mappings. */
  providers: RouteProvider[]
}

/** Request payload to add a provider assignment to a route. */
export interface AddRouteProviderRequest {
  /** Target backend ID. */
  backend_id: string
  /** Supported models list. */
  models?: string[]
  /** Priority position. */
  priority?: number
  /** Active status. */
  enabled?: boolean
}

/** Request payload to update a provider assignment configuration. */
export interface UpdateRouteProviderRequest {
  /** Supported models list. */
  models?: string[]
  /** Priority position. */
  priority?: number
  /** Active status. */
  enabled?: boolean
}

/** Request payload to reorder provider mappings. */
export interface ReorderRouteProvidersRequest {
  /** Ordered list of provider assignment IDs. */
  provider_ids: string[]
}
