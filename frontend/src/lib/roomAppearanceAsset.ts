const ROOM_APPEARANCE_ASSET_PATTERN = /^ra_[0-9a-f]{32}$/;
const ROOM_APPEARANCE_REFERENCE_PATTERN =
  /^\/api\/attachments\/(ra_[0-9a-f]{32})\?view=1$/;

export type RoomAppearanceAssetReference = Readonly<{
  assetId: string;
  url: string;
}>;

export function roomAppearanceAssetId(value: string): string {
  if (!ROOM_APPEARANCE_ASSET_PATTERN.test(value)) {
    throw new Error("방 외형 자산 식별자가 올바르지 않습니다.");
  }
  return value;
}

export function roomAppearanceAssetReference(
  value: string
): RoomAppearanceAssetReference {
  const match = ROOM_APPEARANCE_REFERENCE_PATTERN.exec(value);
  if (!match) {
    throw new Error("방 외형 자산 참조가 올바르지 않습니다.");
  }
  return Object.freeze({ assetId: roomAppearanceAssetId(match[1]), url: value });
}
