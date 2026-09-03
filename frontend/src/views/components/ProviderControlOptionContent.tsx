import type { ProviderControlOption } from "../../roomSocketClient";
import { providerControlOptionDescription } from "./providerModelOptions";

export default function ProviderControlOptionContent({
  option,
  showDescription = false,
  contextBadge = "",
  pricingOnly = false,
}: {
  option: ProviderControlOption;
  showDescription?: boolean;
  contextBadge?: string;
  pricingOnly?: boolean;
}) {
  const badges = pricingOnly ? pricingBadges(option) : optionBadges(option);
  const description = showDescription ? providerControlOptionDescription(option) : "";
  return (
    <span className="dc-agent-select-option-content">
      <span className="dc-agent-select-option-copy">
        <span className="truncate preserve-words">{option.label}</span>
        {description && (
          <small className="truncate preserve-words">{description}</small>
        )}
      </span>
      <span className="dc-agent-select-option-trailing">
        {contextBadge && <small className="dc-agent-select-context-badge">{contextBadge}</small>}
        {providerControlOptionEffect(option) === "ultra" && (
          <small className="dc-agent-select-ultra-badge">Ultra</small>
        )}
        {badges.length > 0 && (
          <span className="dc-agent-select-badges">
            {badges.map((badge) => (
              <small key={badge}>{badge}</small>
            ))}
          </span>
        )}
      </span>
    </span>
  );
}

export function providerControlOptionEffect(option?: ProviderControlOption): string {
  return option?.metadata?.effect === "ultra" ? "ultra" : "";
}

export function providerControlOptionHasDescription(option: ProviderControlOption): boolean {
  return Boolean(providerControlOptionDescription(option));
}

export function providerControlOptionAccessibleName(option: ProviderControlOption): string {
  const description = providerControlOptionDescription(option);
  return [option.label, ...optionBadges(option), description].filter(Boolean).join(" ");
}

function optionBadges(option: ProviderControlOption): string[] {
  const badges: string[] = [];
  const configured = option.metadata?.badges;
  if (Array.isArray(configured)) {
    for (const value of configured) {
      if (typeof value === "string" && value.trim()) badges.push(value.trim());
    }
  }
  if (option.metadata?.pricing === "free") badges.push("Free");
  if (option.metadata?.pricing === "free_tier") badges.push("Free tier");
  if (option.metadata?.execution_location === "cloud") badges.push("Cloud");
  if (option.metadata?.execution_location === "local") badges.push("Local");
  if (option.metadata?.vision === true) badges.push("Vision");
  if (option.metadata?.reasoning === true) badges.push("Reasoning");
  return [...new Set(badges)];
}

function pricingBadges(option: ProviderControlOption): string[] {
  if (option.metadata?.pricing === "free") return ["Free"];
  if (option.metadata?.pricing === "free_tier") return ["Free tier"];
  return [];
}
