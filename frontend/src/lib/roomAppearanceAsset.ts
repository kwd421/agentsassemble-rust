import {
  ROOM_APPEARANCE_ASSET_HEX_LENGTH,
  ROOM_APPEARANCE_ASSET_PREFIX,
  ROOM_APPEARANCE_REFERENCE_PREFIX,
  ROOM_APPEARANCE_REFERENCE_SUFFIX,
} from "../types/generated/ROOM_APPEARANCE_WIRE";

export type RoomAppearanceAssetReference = Readonly<{
  assetId: string;
  url: string;
}>;

export function roomAppearanceAssetId(value: string): string {
  const hex = value.startsWith(ROOM_APPEARANCE_ASSET_PREFIX)
    ? value.slice(ROOM_APPEARANCE_ASSET_PREFIX.length)
    : "";
  if (
    hex.length !== ROOM_APPEARANCE_ASSET_HEX_LENGTH ||
    ![...hex].every((character) =>
      (character >= "0" && character <= "9") ||
      (character >= "a" && character <= "f")
    )
  ) {
    throw new Error("방 외형 자산 식별자가 올바르지 않습니다.");
  }
  return value;
}

export function roomAppearanceAssetReference(
  value: string
): RoomAppearanceAssetReference {
  if (
    !value.startsWith(ROOM_APPEARANCE_REFERENCE_PREFIX) ||
    !value.endsWith(ROOM_APPEARANCE_REFERENCE_SUFFIX)
  ) {
    throw new Error("방 외형 자산 참조가 올바르지 않습니다.");
  }
  const assetId = value.slice(
    ROOM_APPEARANCE_REFERENCE_PREFIX.length,
    value.length - ROOM_APPEARANCE_REFERENCE_SUFFIX.length
  );
  return Object.freeze({ assetId: roomAppearanceAssetId(assetId), url: value });
}
