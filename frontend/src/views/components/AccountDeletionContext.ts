import { createContext } from "react";
/** Central account UI lifetime is above room/native startup authority changes. */
export const AccountDeletionContext = createContext<(() => void) | null>(null);
