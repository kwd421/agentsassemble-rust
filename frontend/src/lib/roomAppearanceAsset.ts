const ROOM_APPEARANCE_ASSET_PATTERN = /^ra_[0-9a-f]{32}$/;

export function roomAppearanceAssetId(value: string): string {
  if (!ROOM_APPEARANCE_ASSET_PATTERN.test(value)) {
    throw new Error("방 외형 자산 식별자가 올바르지 않습니다.");
  }
  return value;
}
