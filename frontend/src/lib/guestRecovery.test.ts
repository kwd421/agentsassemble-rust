import { afterEach, describe, expect, it } from "vitest";

import {
  consumeGuestRecoveryRequestFromUrl,
  guestRecoveryRequestFromUrl,
} from "./guestRecovery";

describe("consumeGuestRecoveryRequestFromUrl", () => {
  afterEach(() => {
    window.history.replaceState({}, "", "/");
  });

  it("captures an opaque case-sensitive recovery request and removes its secret from browser history", () => {
    window.history.replaceState(
      {},
      "",
      "/recover?recover=1&room=friend-room#recovery=aagr1.AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-abcdEfA"
    );

    expect(consumeGuestRecoveryRequestFromUrl()).toEqual({
      recoveryCode: "aagr1.AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-abcdEfA",
      roomId: "friend-room",
    });
    expect(window.location.href).not.toContain("recovery=");
    expect(window.location.href).not.toContain("friend-room");
  });

  it("recognizes an authorized recovery entrance without consuming it", () => {
    const url =
      "http://localhost/recover?recover=1&room=friend-room#recovery=aagr1.AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-abcdEfA";
    const currentUrl = window.location.href;

    expect(guestRecoveryRequestFromUrl(url)).toEqual({
      recoveryCode: "aagr1.AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-abcdEfA",
      roomId: "friend-room",
    });
    expect(window.location.href).toBe(currentUrl);
  });
});
