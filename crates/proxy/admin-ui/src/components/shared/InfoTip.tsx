/** Tiny "(?)" marker with a native tooltip. Dependency-free — uses the browser's
 *  title attribute (same approach as the uptime day cells). For richer hover cards,
 *  swap the implementation here; every call site stays the same. */
export default function InfoTip({ text }: { text: string }) {
  return (
    <span className="info-tip" title={text} aria-label={text} role="img">
      ?
    </span>
  )
}
