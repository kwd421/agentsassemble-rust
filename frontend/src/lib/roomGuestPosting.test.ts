import { describe, expect, it } from "vitest";
import { messageAttachmentAuthorityForRoom } from "./roomGuestPosting";

describe("messageAttachmentAuthorityForRoom", () => {
  it("routes a remote room through its own session, never the local host", () => {
    expect(
      messageAttachmentAuthorityForRoom({
        roomOrigin: "remote_server",
        postingMode: "guest",
        sessionToken: "session",
        deviceToken: "device",
      })
    ).toEqual({ kind: "remote", sessionToken: "session", deviceToken: "device" });
  });

  it("leaves a session-less remote room without authority instead of falling back to local", () => {
    expect(
      messageAttachmentAuthorityForRoom({
        roomOrigin: "remote_server",
        postingMode: "host",
        sessionToken: "",
        deviceToken: "device",
      })
    ).toBeUndefined();
    expect(
      messageAttachmentAuthorityForRoom({
        roomOrigin: "remote_server",
        postingMode: "guest",
        sessionToken: "",
        deviceToken: "device",
      })
    ).toBeUndefined();
  });

  it("keeps local rooms on the local authority", () => {
    expect(
      messageAttachmentAuthorityForRoom({
        roomOrigin: "local",
        postingMode: "host",
        sessionToken: "",
        deviceToken: "device",
      })
    ).toEqual({ kind: "local" });
    expect(
      messageAttachmentAuthorityForRoom({
        roomOrigin: "local",
        postingMode: "guest",
        sessionToken: "local-session",
        deviceToken: "device",
      })
    ).toEqual({ kind: "remote", sessionToken: "local-session", deviceToken: "device" });
  });
});
