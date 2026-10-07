import { readResourceBlob, releaseResourceBlob } from "../lib/remote/remoteResources";
import { MAX_ATTACHMENT_BYTES } from "../types/generated/ASSET_SAFETY_WIRE";
import { isPrivateNoStoreResponse } from "./http";

const PNG_SIGNATURE = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);

export async function strictPrivatePngBlob(
  response: Response,
  invalidMessage: string
): Promise<Blob> {
  if (!isPrivateNoStoreResponse(response, "image/png")) {
    throw new Error(invalidMessage);
  }
  const blob = await readResourceBlob(response);
  if (blob.size < PNG_SIGNATURE.length || blob.size > MAX_ATTACHMENT_BYTES) {
    releaseResourceBlob(blob); throw new Error(invalidMessage);
  }
  const signature = new Uint8Array(
    await blob.slice(0, PNG_SIGNATURE.length).arrayBuffer()
  );
  if (signature.some((byte, index) => byte !== PNG_SIGNATURE[index])) {
    releaseResourceBlob(blob); throw new Error(invalidMessage);
  }
  return blob;
}
