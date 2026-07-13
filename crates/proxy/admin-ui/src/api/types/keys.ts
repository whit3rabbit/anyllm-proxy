/** Represents a virtual API key configuration and its associated limits. */
export interface VirtualKey {
  /** Unique database ID. */
  id: number
  /** The prefix of the key shown to users. */
  key_prefix: string
  /** Optional descriptive note. */
  description: string | null
  /** ISO 8601 creation timestamp. */
  created_at: string
  /** ISO 8601 expiration timestamp, if set. */
  expires_at: string | null
  /** ISO 8601 revocation timestamp, if revoked. */
  revoked_at: string | null
  /** Spend limit in USD. */
  spend_limit: number | null
  /** Monthly budget limit in USD. */
  max_budget_usd: number | null
  /** Duration of the budget period (e.g. 'monthly'). */
  budget_duration: string | null
  /** Requests-per-minute limit. */
  rpm_limit: number | null
  /** Tokens-per-minute limit. */
  tpm_limit: number | null
  /** Total spend in USD across the key lifetime. */
  total_spend: number
  /** Total count of requests made with this key. */
  total_requests: number
  /** Total count of tokens processed. */
  total_tokens: number
  /** ISO 8601 reset timestamp for the current budget period. */
  period_reset_at: string | null
  /** List of model names this key is restricted to, if any. */
  allowed_models: string[] | null
  /** List of route names this key is restricted to, if any. */
  allowed_routes: string[] | null
  /** Active status of the key. */
  status: 'active' | 'revoked' | 'expired' | 'override'
  /** Spend in USD during the current budget period. */
  period_spend_usd: number
}

/** Spent token and requests details for a virtual key. */
export interface KeySpend {
  /** Unique key database ID. */
  id: number
  /** Total spend in USD. */
  total_spend: number
  /** Total request count. */
  total_requests: number
  /** Total token count. */
  total_tokens: number
}
