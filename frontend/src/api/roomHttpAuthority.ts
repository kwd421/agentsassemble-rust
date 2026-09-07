export type RoomHttpAuthority =
  | { kind: "local" }
  | { kind: "remote"; sessionToken: string; deviceToken?: string };

export function resolveRoomHttpAuthority(
  sessionToken: string,
  localAvailable: boolean,
  deviceToken?: string
): RoomHttpAuthority | undefined {
  if (sessionToken) return { kind: "remote", sessionToken, deviceToken };
  return localAvailable ? { kind: "local" } : undefined;
}
