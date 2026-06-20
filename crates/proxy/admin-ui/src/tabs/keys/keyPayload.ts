interface CreateKeyPayloadInput {
  description: string
  spendLimit: string
  rpmLimit: string
}

function optionalNumber(raw: string): number | null {
  const trimmed = raw.trim()
  return trimmed ? Number(trimmed) : null
}

export function buildCreateKeyPayload(input: CreateKeyPayloadInput): Record<string, unknown> {
  return {
    description: input.description.trim() || null,
    max_budget_usd: optionalNumber(input.spendLimit),
    rpm_limit: optionalNumber(input.rpmLimit),
  }
}
