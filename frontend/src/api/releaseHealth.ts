import { fetchJsonServerOperator } from "./http";
import type { ReleaseHealthCheck } from "../types/generated/ReleaseHealthCheck";
import type { ReleaseHealthReport } from "../types/generated/ReleaseHealthReport";

export async function readReleaseHealth() {
  const [checks, report] = await Promise.all([
    fetchJsonServerOperator<ReleaseHealthCheck[]>("/api/release-health"),
    fetchJsonServerOperator<ReleaseHealthReport | null>("/api/release-health/queue"),
  ]);
  return { checks, report };
}
