type AttachmentReference = {
  value: string;
  disposition: "view" | "download";
};

const ATTACHMENT_REFERENCE =
  /^\/api\/attachments\/(?:[A-Za-z0-9_-]{8,64})\?(view|download)=1$/;

export function parseAttachmentReference(value: unknown): AttachmentReference | null {
  if (typeof value !== "string") return null;
  const match = ATTACHMENT_REFERENCE.exec(value);
  if (!match) return null;
  return {
    value,
    disposition: match[1] as AttachmentReference["disposition"],
  };
}

export function profileAvatarReference(value: unknown): string | undefined {
  if (value === "" || value === undefined) return undefined;
  const reference = parseAttachmentReference(value);
  if (!reference || reference.disposition !== "view") {
    throw new Error("프로필 아바타 참조가 현재 계약과 일치하지 않습니다.");
  }
  return reference.value;
}

export function resolveAttachmentReference(
  value: string | undefined,
  resourceBase: string
): string | undefined {
  if (!value) return undefined;
  const reference = parseAttachmentReference(value);
  if (!reference) return undefined;
  let base: URL;
  try {
    base = new URL(resourceBase);
  } catch {
    return undefined;
  }
  if (
    !["http:", "https:"].includes(base.protocol) ||
    base.username ||
    base.password ||
    base.origin !== resourceBase
  ) {
    return undefined;
  }
  return `${base.origin}${reference.value}`;
}
