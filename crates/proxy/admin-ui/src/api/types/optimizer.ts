/** Status of the optional LLMLingua-2 ONNX model artifact (optimizer scorer tier). */
export interface OptimizerModelStatus {
  /** Proxy built with the `optimizer-onnx` feature. When false the tier is inert. */
  compiled_in: boolean
  /** Verified model artifact is present on disk. */
  present: boolean
  /** A download+verify is currently in flight. */
  downloading: boolean
  /** Last download error, if any. */
  error: string | null
  /** Pinned sha256 the artifact is verified against. */
  sha256: string
  /** Expected download size in bytes. */
  size_bytes: number
}
