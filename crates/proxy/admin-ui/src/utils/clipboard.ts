/** Copy text to the clipboard. Returns false if the browser blocks it
 *  (e.g. non-secure context). localhost is treated as secure, so this works
 *  for the admin UI on http://127.0.0.1. */
export async function copyToClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    return false
  }
}
