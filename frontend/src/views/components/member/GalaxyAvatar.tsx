import type { CSSProperties, ReactNode } from "react";
import "./GalaxyAvatar.css";

/**
 * The avatar wrapper for maximum-reasoning agents. The avatar itself stays on
 * top; the nebula, stars and meteor live behind it in pure CSS layers.
 */
export default function GalaxyAvatar({
  size,
  children,
  className = "",
}: {
  size: number;
  children: ReactNode;
  className?: string;
}) {
  const full = size >= 40;
  return (
    <span
      className={`galaxy-avatar${className ? ` ${className}` : ""}`}
      style={{ "--galaxy-size": `${size}px` } as CSSProperties}
      data-galaxy-size={full ? "full" : "compact"}
    >
      <span className="galaxy-avatar-nebula" aria-hidden="true" />
      <span
        className="galaxy-avatar-stars galaxy-avatar-stars--outer"
        aria-hidden="true"
      />
      <span
        className="galaxy-avatar-stars galaxy-avatar-stars--inner"
        aria-hidden="true"
      />
      {full && <span className="galaxy-avatar-meteor" aria-hidden="true" />}
      <span className="galaxy-avatar-core">{children}</span>
    </span>
  );
}
