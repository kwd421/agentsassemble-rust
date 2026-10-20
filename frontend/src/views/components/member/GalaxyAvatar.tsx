import type { CSSProperties, ReactNode } from "react";
import "./GalaxyAvatar.css";

type BrightStar = {
  angle: number;
  radius: number;
  size: number;
  delay: string;
  duration: string;
};

// The bright stars orbit with the arms. Their staggered timings make a few
// flare at a time instead of the whole field pulsing together.
const BRIGHT_STARS: BrightStar[] = [
  { angle: 30, radius: 0.58, size: 2.8, delay: "-0.4s", duration: "3.2s" },
  { angle: 120, radius: 0.65, size: 3, delay: "-1.6s", duration: "2.6s" },
  { angle: 210, radius: 0.56, size: 2.6, delay: "-2.1s", duration: "3.8s" },
  { angle: 300, radius: 0.68, size: 3.2, delay: "-0.9s", duration: "2.9s" },
];

/**
 * The avatar wrapper for maximum-reasoning agents. Everything the galaxy draws
 * lives in a circular, radially masked halo behind the avatar; the avatar and
 * its controls stay on top and keep their own pointer behavior.
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
  // Four bright stars at even angles keep the glow balanced at both sizes.
  const stars = BRIGHT_STARS;
  return (
    <span
      className={`galaxy-avatar${className ? ` ${className}` : ""}`}
      style={{ "--galaxy-size": `${size}px` } as CSSProperties}
      data-galaxy-size={full ? "full" : "compact"}
    >
      <span className="galaxy-avatar-field" aria-hidden="true">
        <span className="galaxy-avatar-glow" />
        <span className="galaxy-avatar-arm galaxy-avatar-arm--a" />
        <span className="galaxy-avatar-arm galaxy-avatar-arm--b" />
        <span className="galaxy-avatar-stars galaxy-avatar-stars--a">
          {stars.map((star) => (
            <span
              key={star.angle}
              className="galaxy-avatar-bright"
              style={{
                "--star-angle": `${star.angle}deg`,
                "--star-radius": star.radius,
                "--star-size": `${star.size}px`,
                "--star-delay": star.delay,
                "--star-duration": star.duration,
              } as CSSProperties}
            />
          ))}
        </span>
        <span className="galaxy-avatar-stars galaxy-avatar-stars--b" />
        {full && <span className="galaxy-avatar-meteor" />}
      </span>
      <span className="galaxy-avatar-ring" aria-hidden="true" />
      <span className="galaxy-avatar-core">{children}</span>
    </span>
  );
}
