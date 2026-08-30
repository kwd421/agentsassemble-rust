export type RoomHttpAuthority =
  | { kind: "local" }
  | { kind: "remote"; sessionToken: string };

export function resolveRoomHttpAuthority(
  sessionToken: string,
  localAvailable: boolean
): RoomHttpAuthority | undefined {
  if (sessionToken) return { kind: "remote", sessionToken };
  return localAvailable ? { kind: "local" } : undefined;
}
