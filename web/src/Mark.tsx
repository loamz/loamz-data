/** The Loamz mark: two rounded squares, violet and orange. The same shape and colours as in Chat. */
export function Mark({ size = 28, className = "" }: { size?: number; className?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden="true" focusable="false" className={`shrink-0 ${className}`}>
      <rect x="2" y="2" width="13" height="13" rx="2.5" fill="var(--accent)" transform="rotate(8 8 8)" />
      <rect x="9" y="9" width="13" height="13" rx="2.5" fill="var(--accent-2)" transform="rotate(8 15 15)" />
    </svg>
  );
}
