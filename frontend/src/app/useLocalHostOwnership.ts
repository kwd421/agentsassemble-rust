import { useEffect, useState } from "react";
import { isLocalOwnedByAccount } from "../lib/central/localOwnership";
import {
  ensureDesktopRuntimeStarted,
  isDesktopWebview,
  requestDesktopHostDeviceInfo,
} from "../lib/desktopBridge";
import type { RoomDockItem } from "../lib/roomDockModel";
import type { CentralDirectoryState } from "./useCentralDirectory";

/**
 * Registration ownership for this computer's server. The host id comes
 * from device info after the gate-independent runtime start; it is never
 * derived from whether rooms loaded (they are absent while the runtime
 * starts). While unknown the local rooms are not mine, so the signed-in
 * central account is shown instead of a foreign operator.
 */
export function useLocalHostOwnership(
  directoryRooms: RoomDockItem[],
  centralDirectory: CentralDirectoryState | null,
): { localHostServerId: string | null | undefined; localOwnedByAccount: boolean } {
  const [localHostServerId, setLocalHostServerId] = useState<string | null | undefined>(undefined);
  const centralPersonId = centralDirectory?.person?.person_id;
  useEffect(() => {
    if (!isDesktopWebview() || !centralPersonId) {
      setLocalHostServerId(undefined);
      return;
    }
    let active = true;
    setLocalHostServerId(undefined);
    void ensureDesktopRuntimeStarted()
      .then(() => requestDesktopHostDeviceInfo())
      .then((device) => {
        if (active) setLocalHostServerId(device.server_id ?? null);
      })
      .catch(() => {
        if (active) setLocalHostServerId(undefined);
      });
    return () => {
      active = false;
    };
  }, [centralPersonId]);
  const localOwnedByAccount = isLocalOwnedByAccount(directoryRooms, centralDirectory, localHostServerId);
  return { localHostServerId, localOwnedByAccount };
}
