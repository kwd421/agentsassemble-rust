import type { CSSProperties } from "react";

type Star = {
  left: number;
  top: number;
  size: number;
  bright: boolean;
  delay: number;
  duration: number;
};

// Small seeded generator: each agent keeps the same sky across renders.
function seeded(seed: string) {
  let state = 2166136261;
  for (const character of seed) state = Math.imul(state ^ character.charCodeAt(0), 16777619);
  return () => {
    state = Math.imul(state ^ (state >>> 15), 2246822507);
    state = Math.imul(state ^ (state >>> 13), 3266489909);
    state ^= state >>> 16;
    return (state >>> 0) / 4294967296;
  };
}

function starsFor(seed: string, count: number, brightCount: number): Star[] {
  const random = seeded(seed);
  return Array.from({ length: count }, (_, index) => ({
    left: random() * 100,
    top: 8 + random() * 84,
    size: index < brightCount ? 8 + random() * 3 : 1.3 + random() * 1.2,
    bright: index < brightCount,
    delay: -random() * 6,
    duration: 2.4 + random() * 4,
  }));
}

/** Background sky for maximum-reasoning agents; sits behind the row content. */
export default function GalaxySky({ seed, count = 12 }: { seed: string; count?: number }) {
  const stars = starsFor(seed, count, 2);
  const layer = stars.map((star, index) => (
    <span
      key={index}
      className={star.bright ? "galaxy-star galaxy-star--bright" : "galaxy-star"}
      style={{
        left: `${star.left}%`,
        top: `${star.top}%`,
        width: star.size,
        height: star.size,
        animationDelay: `${star.delay}s`,
        animationDuration: `${star.duration}s`,
      } as CSSProperties}
    />
  ));
  return (
    <span className="galaxy-sky" aria-hidden="true">
      {/* Two copies side by side drift left as one seamless band. */}
      <span className="galaxy-sky-band">
        <span className="galaxy-sky-copy">{layer}</span>
        <span className="galaxy-sky-copy">{layer}</span>
      </span>
    </span>
  );
}
