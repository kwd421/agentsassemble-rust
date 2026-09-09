import { fetchJsonServerOperator } from "./http";
import type { LocalResourceStatus } from "../types/generated/LocalResourceStatus";
export type { LocalResourceStatus };

export function fetchLocalResources() {
  return fetchJsonServerOperator<LocalResourceStatus>("/api/local-resources");
}
