/** Represents a backend endpoint model and its metrics. */
export interface Backend {
  /** Name of the backend. */
  name: string
  /** Model mapped for heavy workloads. */
  big_model: string
  /** Model mapped for light workloads. */
  small_model: string
  /** Request outcome counters. */
  metrics: {
    requests_total: number
    requests_success: number
    requests_error: number
  }
}

/** Represents a backend credentials deployment managed by the admin. */
export interface ManagedBackend {
  /** Unique database ID. */
  id: string
  /** Unique backend name. */
  name: string
  /** Provider ID. */
  provider_id: string
  /** True if the API key is configured. */
  api_key_set: boolean
  /** True if AWS credentials are set (for AWS Bedrock, etc.). */
  aws_creds_set: boolean
  /** Base URL of the API endpoint. */
  api_base: string | null
  /** Optional deployment name (e.g. Azure deployment name). */
  deployment: string | null
  /** Optional API version. */
  api_version: string | null
  /** Optional cloud project ID. */
  project: string | null
  /** Optional cloud region (e.g. AWS/Azure region). */
  region: string | null
  /** Rate limit: requests per minute override. */
  rpm: number | null
  /** Rate limit: tokens per minute override. */
  tpm: number | null
  /** True if this backend is enabled. */
  enabled: boolean
  /** ISO 8601 creation timestamp. */
  created_at: string
  /** ISO 8601 last update timestamp. */
  updated_at: string
}

/** Response containing managed backends. */
export interface ManagedBackendsResponse {
  /** List of managed backends. */
  backends: ManagedBackend[]
}

/** Request payload for creating a new managed backend. */
export interface CreateManagedBackendRequest {
  /** Unique name for the backend. */
  name: string
  /** Catalog provider ID. */
  provider_id: string
  /** API key credential string. */
  api_key?: string
  /** API base URL. */
  api_base?: string
  /** API deployment name. */
  deployment?: string
  /** API version. */
  api_version?: string
  /** Cloud project ID. */
  project?: string
  /** Cloud region. */
  region?: string
  /** AWS access key ID. */
  aws_access_key_id?: string
  /** AWS secret access key. */
  aws_secret_access_key?: string
  /** AWS session token. */
  aws_session_token?: string
  /** Requests per minute limit. */
  rpm?: number
  /** Tokens per minute limit. */
  tpm?: number
  /** Active status toggle. */
  enabled?: boolean
}

/** Request payload for updating an existing managed backend. */
export type UpdateManagedBackendRequest = Partial<Omit<CreateManagedBackendRequest, 'name'>>
