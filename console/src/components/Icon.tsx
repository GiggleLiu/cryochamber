/**
 * The app's icon family: one grid (24×24), one stroke weight, round joins.
 * Inline SVG rather than an icon font or package — zero bytes of dependency
 * and the strokes inherit `currentColor` from whatever button holds them.
 */
type IconProps = { size?: number; className?: string }

function Svg({ size = 24, className, children }: IconProps & { children: React.ReactNode }) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {children}
    </svg>
  )
}

export const ChevronLeft = (p: IconProps) => (
  <Svg {...p}><path d="M15 5l-7 7 7 7" /></Svg>
)

export const ArrowDown = (p: IconProps) => (
  <Svg {...p}><path d="M12 5v14M6 13l6 6 6-6" /></Svg>
)

export const ArrowUp = (p: IconProps) => (
  <Svg {...p}><path d="M12 19V5M6 11l6-6 6 6" /></Svg>
)

export const Gear = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="3.1" />
    <path d="M19.6 14.2a1.5 1.5 0 0 0 .3 1.65l.05.06a1.8 1.8 0 1 1-2.55 2.55l-.06-.06a1.5 1.5 0 0 0-1.65-.3 1.5 1.5 0 0 0-.91 1.37v.17a1.8 1.8 0 0 1-3.6 0v-.09a1.5 1.5 0 0 0-.98-1.37 1.5 1.5 0 0 0-1.65.3l-.06.06a1.8 1.8 0 1 1-2.55-2.55l.06-.06a1.5 1.5 0 0 0 .3-1.65 1.5 1.5 0 0 0-1.37-.91h-.17a1.8 1.8 0 0 1 0-3.6h.09a1.5 1.5 0 0 0 1.37-.98 1.5 1.5 0 0 0-.3-1.65l-.06-.06a1.8 1.8 0 1 1 2.55-2.55l.06.06a1.5 1.5 0 0 0 1.65.3h.07a1.5 1.5 0 0 0 .91-1.37v-.17a1.8 1.8 0 0 1 3.6 0v.09a1.5 1.5 0 0 0 .91 1.37 1.5 1.5 0 0 0 1.65-.3l.06-.06a1.8 1.8 0 1 1 2.55 2.55l-.06.06a1.5 1.5 0 0 0-.3 1.65v.07a1.5 1.5 0 0 0 1.37.91h.17a1.8 1.8 0 0 1 0 3.6h-.09a1.5 1.5 0 0 0-1.37.91z" />
  </Svg>
)

export const Paperclip = (p: IconProps) => (
  <Svg {...p}>
    <path d="M20.4 11.6l-8.5 8.5a5 5 0 0 1-7.07-7.07l8.49-8.49a3.33 3.33 0 1 1 4.71 4.71l-8.48 8.49a1.67 1.67 0 1 1-2.36-2.36l7.78-7.78" />
  </Svg>
)

export const Close = (p: IconProps) => (
  <Svg {...p}><path d="M18 6L6 18M6 6l12 12" /></Svg>
)

export const AlertCircle = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 7.5v5" />
    <circle cx="12" cy="16.2" r="0.9" fill="currentColor" stroke="none" />
  </Svg>
)

export const Inbox = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3 13h4.5l1.6 2.6h5.8L16.5 13H21" />
    <path d="M5.4 5h13.2l2.4 8v4.6A2.4 2.4 0 0 1 18.6 20H5.4A2.4 2.4 0 0 1 3 17.6V13z" />
  </Svg>
)

export const Message = (p: IconProps) => (
  <Svg {...p}>
    <path d="M20.5 11.6a7.9 7.9 0 0 1-8.5 7.9 9 9 0 0 1-2.6-.4L4.5 21l1.4-4.1a7.7 7.7 0 0 1-1.4-4.4 8 8 0 0 1 8.5-7.9 8 8 0 0 1 7.5 7z" />
  </Svg>
)

/** Overflow / "more actions" — the conversation header's Controls button. */
export const Dots = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="5" cy="12" r="1.4" fill="currentColor" stroke="none" />
    <circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none" />
    <circle cx="19" cy="12" r="1.4" fill="currentColor" stroke="none" />
  </Svg>
)

export const UserPlus = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="9.5" cy="8" r="3.5" />
    <path d="M3 20a6.5 6.5 0 0 1 13 0" />
    <path d="M19 8.5v5M16.5 11h5" />
  </Svg>
)

export const Plus = (p: IconProps) => (
  <Svg {...p}><path d="M12 5v14M5 12h14" /></Svg>
)

/**
 * The product mark: a chat bubble containing a shell prompt. The app is a
 * conversation you type commands into, so the glyph says exactly that — and
 * its tail is the bubble tail the message list deliberately does not draw.
 */
export function Logo({ size = 56, className }: IconProps) {
  // The cryochamber mark: concentric shells cooling inward, from the outer
  // casing to the frozen core. Kept in its own palette rather than
  // `currentColor` — it is a brand mark, not an icon.
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 100 100"
      aria-hidden="true"
      focusable="false"
    >
      <circle cx="50" cy="50" r="45" fill="#1a2744" />
      <circle cx="50" cy="48.75" r="40.5" fill="#ffffff" />
      <circle cx="50" cy="47.5" r="37" fill="#2d5a8e" />
      <circle cx="50" cy="43.75" r="26.25" fill="#5ba3d9" />
      <circle cx="50" cy="41.25" r="15" fill="#a8d8ea" />
      <circle cx="50" cy="40.5" r="6.25" fill="#e0f0f8" />
    </svg>
  )
}

