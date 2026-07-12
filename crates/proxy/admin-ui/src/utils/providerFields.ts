import { CatalogProvider } from '../api/types'

export type FieldGroup = 'auth' | 'endpoint' | 'limits'

export interface FieldDef {
  name: string
  label: string
  type: 'text' | 'password' | 'url' | 'number'
  required: boolean
  placeholder?: string
  hint?: string
  group: FieldGroup
}

// Mirror of resolve_discover_target's /v1/models logic in models.rs — keep in sync.
// Handles trailing slashes and avoids doubling /v1 (catalog defaults end in /v1).
export function resolveDiscoveryUrl(base: string): string {
  const trimmed = base.trim().replace(/\/+$/, '')
  if (!trimmed) return ''
  if (trimmed.endsWith('/models')) return trimmed
  if (trimmed.endsWith('/v1')) return `${trimmed}/models`
  return `${trimmed}/v1/models`
}

export function getProviderFields(provider: CatalogProvider): FieldDef[] {
  const fields: FieldDef[] = []
  const firstEnvVar = provider.env_vars.length > 0 ? provider.env_vars[0] : null

  const { protocol, auth, default_base_url } = provider

  if (protocol === 'openai_compat' && auth === 'bearer') {
    fields.push({
      name: 'api_key',
      label: 'API Key',
      type: 'password',
      required: true,
      group: 'auth',
      ...(firstEnvVar ? { hint: `or set ${firstEnvVar} env var` } : {}),
    })
    fields.push({
      name: 'api_base',
      label: 'API Base URL',
      type: 'url',
      required: !default_base_url,
      group: 'endpoint',
      ...(default_base_url ? { placeholder: default_base_url } : {}),
    })
  } else if (protocol === 'openai_compat' && auth === 'none') {
    fields.push({
      name: 'api_base',
      label: 'API Base URL',
      type: 'url',
      required: !default_base_url,
      group: 'endpoint',
      ...(default_base_url ? { placeholder: default_base_url } : {}),
    })
    // Local servers (LM Studio/Ollama/vLLM) can optionally enforce a key.
    fields.push({
      name: 'api_key',
      label: 'API Key (optional)',
      type: 'password',
      required: false,
      group: 'auth',
      hint: 'Only if your local server enforces a key',
    })
  } else if (protocol === 'azure_openai' && auth === 'azure_api_key') {
    fields.push({
      name: 'api_key',
      label: 'Azure API Key',
      type: 'password',
      required: true,
      group: 'auth',
    })
    fields.push({
      name: 'api_base',
      label: 'Endpoint URL',
      type: 'url',
      required: true,
      placeholder: 'https://<resource>.openai.azure.com',
      group: 'endpoint',
    })
    fields.push({
      name: 'deployment',
      label: 'Deployment Name',
      type: 'text',
      required: true,
      group: 'endpoint',
    })
    fields.push({
      name: 'api_version',
      label: 'API Version',
      type: 'text',
      required: true,
      placeholder: '2024-08-01-preview',
      group: 'endpoint',
    })
  } else if (protocol === 'vertex_ai' && auth === 'google_api_key') {
    fields.push({
      name: 'api_key',
      label: 'API Key',
      type: 'password',
      required: true,
      group: 'auth',
    })
    fields.push({
      name: 'project',
      label: 'GCP Project ID',
      type: 'text',
      required: true,
      group: 'endpoint',
    })
    fields.push({
      name: 'region',
      label: 'GCP Region',
      type: 'text',
      required: true,
      placeholder: 'us-central1',
      group: 'endpoint',
    })
  } else if (protocol === 'bedrock_native' && auth === 'aws_sigv4') {
    fields.push({
      name: 'aws_access_key_id',
      label: 'AWS Access Key ID',
      type: 'text',
      required: true,
      group: 'auth',
    })
    fields.push({
      name: 'aws_secret_access_key',
      label: 'AWS Secret Access Key',
      type: 'password',
      required: true,
      group: 'auth',
    })
    fields.push({
      name: 'aws_session_token',
      label: 'AWS Session Token',
      type: 'password',
      required: false,
      group: 'auth',
    })
    fields.push({
      name: 'region',
      label: 'AWS Region',
      type: 'text',
      required: true,
      placeholder: 'us-east-1',
      group: 'endpoint',
    })
  } else if ((protocol === 'gemini_openai' || protocol === 'gemini_native') && auth === 'google_api_key') {
    fields.push({
      name: 'api_key',
      label: 'API Key',
      type: 'password',
      required: true,
      group: 'auth',
      ...(firstEnvVar ? { hint: `or set ${firstEnvVar} env var` } : {}),
    })
  } else if (protocol === 'anthropic_native' && auth === 'bearer') {
    fields.push({
      name: 'api_key',
      label: 'API Key',
      type: 'password',
      required: true,
      group: 'auth',
      ...(firstEnvVar ? { hint: `or set ${firstEnvVar} env var` } : {}),
    })
  } else {
    // Fallback
    if (auth.includes('bearer')) {
      fields.push({
        name: 'api_key',
        label: 'API Key',
        type: 'password',
        required: true,
        group: 'auth',
      })
    }
    if (auth === 'none' || auth.includes('bearer')) {
      fields.push({
        name: 'api_base',
        label: 'API Base URL',
        type: 'url',
        required: !default_base_url,
        group: 'endpoint',
        ...(default_base_url ? { placeholder: default_base_url } : {}),
      })
    }
    if (auth === 'none') {
      fields.push({
        name: 'api_key',
        label: 'API Key (optional)',
        type: 'password',
        required: false,
        group: 'auth',
        hint: 'Only if your server enforces a key',
      })
    }
  }

  // All providers get rate/token limits
  fields.push({
    name: 'rpm',
    label: 'Rate Limit (req/min)',
    type: 'number',
    required: false,
    group: 'limits',
    hint: 'Stored for reference; not enforced on managed backends',
  })
  fields.push({
    name: 'tpm',
    label: 'Token Limit (tokens/min)',
    type: 'number',
    required: false,
    group: 'limits',
    hint: 'Stored for reference; not enforced on managed backends',
  })

  return fields
}
