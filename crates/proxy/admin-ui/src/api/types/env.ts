/** Warning message generated during env import. */
export interface EnvWarning {
  /** Affected line number. */
  line: number | null
  /** Affected environment variable key. */
  key: string | null
  /** Warning message. */
  message: string
}

/** Response detailing the result of importing an env file. */
export interface EnvImportResponse {
  /** Count of imported variables applied. */
  applied: number
  /** Non-fatal warnings generated. */
  warnings: EnvWarning[]
}

/** Error payload for env file imports. */
export interface EnvImportError {
  /** Hard/blocking validation errors. */
  hard_errors: string[]
  /** Non-fatal warnings. */
  warnings: EnvWarning[]
}
