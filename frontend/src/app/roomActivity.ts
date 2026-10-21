import { createContext, useContext } from "react";

/** External AIs the room's server says are working on a message they received. */
export const RoomActivity = createContext<readonly string[]>([]);

export function useRoomActivity(): readonly string[] {
  return useContext(RoomActivity);
}
