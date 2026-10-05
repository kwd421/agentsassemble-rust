import { createContext } from "react";
import type { CentralOwnerSessionStatus } from "../../types/generated/CentralOwnerSessionStatus";
import type { CentralOwnerWorkspace } from "./ownerWorkspace";

export const CentralOwnerWorkspaceContext = createContext<{
  session: CentralOwnerWorkspace;
  onStatus: (status: CentralOwnerSessionStatus) => void;
} | null>(null);
