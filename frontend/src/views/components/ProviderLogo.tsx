import type { ReactNode } from "react";
import { Bot, Braces } from "lucide-react";
import {
  providerBrandKey,
  resolveProviderPresentation,
  type ProviderBrandKey,
} from "./providerBranding";

export { providerBrandKey };
export type { ProviderBrandKey };

export default function ProviderLogo({
  providerId,
  providerKind,
  size = 24,
  fallback,
  decorative = true,
}: {
  providerId?: string;
  providerKind?: string;
  size?: number;
  fallback?: ReactNode;
  decorative?: boolean;
}) {
  const presentation = resolveProviderPresentation({ providerId, providerKind });
  if (!presentation.brandKey || !presentation.brand) {
    return fallback ?? <Bot size={Math.max(14, Math.round(size * 0.52))} />;
  }
  const { brandKey, brand } = presentation;
  return (
    <span
      className="dc-provider-logo"
      data-provider-brand={brandKey}
      aria-hidden={decorative || undefined}
      aria-label={decorative ? undefined : `${brand.logoLabel} 로고`}
      role={decorative ? undefined : "img"}
      style={{
        width: size,
        height: size,
        background: brand.background,
      }}
    >
      {brand.logo ? (
        <img
          src={brand.logo}
          alt=""
          draggable={false}
          style={{ width: brand.scale, height: brand.scale }}
        />
      ) : (
        <Braces color="#ffffff" size={Math.round(size * 0.58)} strokeWidth={2.2} />
      )}
    </span>
  );
}
