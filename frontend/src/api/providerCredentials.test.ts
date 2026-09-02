import { afterEach, describe, expect, it, vi } from "vitest";

import { requestDesktopHostProductSurface } from "../lib/desktopBridge";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import {
  deleteProviderCredential,
  fetchProviderCredentialStatus,
  setProviderCredential,
} from "./providerCredentials";

const HOST_SURFACE = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "3".repeat(64),
  commands: ["host_product_surface", "runtime_operator_ticket"],
};

const HTTP_BASE_URL = "http://127.0.0.1:49157";

describe("DeepSeek credential HTTP authority", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("uses a fresh operator ticket for status, set, and delete", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce(HOST_SURFACE)
      .mockResolvedValueOnce(ticket("a"))
      .mockResolvedValueOnce(ticket("b"))
      .mockResolvedValueOnce(ticket("c"));
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(statusResponse("missing"))
      .mockResolvedValueOnce(statusResponse("keyring"))
      .mockResolvedValueOnce(statusResponse("missing"));
    vi.stubGlobal("fetch", fetchMock);

    await requestDesktopHostProductSurface();
    await expect(fetchProviderCredentialStatus("deepseek")).resolves.toEqual({
      configured: false,
      source: "missing",
    });
    await expect(
      setProviderCredential("deepseek", "sentinel-provider-value")
    ).resolves.toEqual({ configured: true, source: "keyring" });
    await expect(deleteProviderCredential("deepseek")).resolves.toEqual({
      configured: false,
      source: "missing",
    });

    expect(invoke).toHaveBeenNthCalledWith(1, "host_product_surface");
    expect(invoke).toHaveBeenNthCalledWith(2, "runtime_operator_ticket");
    expect(invoke).toHaveBeenNthCalledWith(3, "runtime_operator_ticket");
    expect(invoke).toHaveBeenNthCalledWith(4, "runtime_operator_ticket");
    expect(fetchMock).toHaveBeenCalledTimes(3);

    const getInit = fetchMock.mock.calls[0]?.[1] as RequestInit;
    const postInit = fetchMock.mock.calls[1]?.[1] as RequestInit;
    const deleteInit = fetchMock.mock.calls[2]?.[1] as RequestInit;
    expect(fetchMock.mock.calls.map((call) => call[0])).toEqual([
      `${HTTP_BASE_URL}/api/provider-credentials/deepseek`,
      `${HTTP_BASE_URL}/api/provider-credentials/deepseek`,
      `${HTTP_BASE_URL}/api/provider-credentials/deepseek`,
    ]);
    expect((getInit.headers as Headers).get("Authorization")).toBe(
      `Bearer ${"a".repeat(64)}`
    );
    expect((getInit.headers as Headers).get("X-Host-Token")).toBeNull();
    expect(postInit).toMatchObject({
      method: "POST",
      body: JSON.stringify({ api_key: "sentinel-provider-value" }),
    });
    expect((postInit.headers as Headers).get("Authorization")).toBe(
      `Bearer ${"b".repeat(64)}`
    );
    expect((postInit.headers as Headers).get("Content-Type")).toBe(
      "application/json"
    );
    expect((postInit.headers as Headers).get("X-Host-Token")).toBeNull();
    expect(deleteInit.method).toBe("DELETE");
    expect(deleteInit.body).toBeUndefined();
    expect((deleteInit.headers as Headers).get("Authorization")).toBe(
      `Bearer ${"c".repeat(64)}`
    );
  });

  it("rejects unimplemented providers before issuing authority", async () => {
    const invoke = vi.fn();
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    await expect(fetchProviderCredentialStatus("opencode")).rejects.toThrow(
      "Unsupported API credential provider: opencode"
    );
    await expect(
      setProviderCredential("openrouter", "sentinel-provider-value")
    ).rejects.toThrow("Unsupported API credential provider: openrouter");
    await expect(deleteProviderCredential("custom_api")).rejects.toThrow(
      "Unsupported API credential provider: custom_api"
    );
    expect(invoke).not.toHaveBeenCalled();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("rejects browser credential operations before network dispatch", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    await expect(fetchProviderCredentialStatus("deepseek")).rejects.toThrow(
      "Provider credential controls require the desktop Rust runtime."
    );
    await expect(
      setProviderCredential("deepseek", "sentinel-provider-value")
    ).rejects.toThrow(
      "Provider credential controls require the desktop Rust runtime."
    );
    await expect(deleteProviderCredential("deepseek")).rejects.toThrow(
      "Provider credential controls require the desktop Rust runtime."
    );
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("rejects retired sources and fields outside the public metadata contract", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce(HOST_SURFACE)
      .mockResolvedValueOnce(ticket("d"))
      .mockResolvedValueOnce(ticket("e"));
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValueOnce(
          new Response(
            JSON.stringify({ configured: true, source: "environment" }),
            { status: 200, headers: { "Content-Type": "application/json" } }
          )
        )
        .mockResolvedValueOnce(
          new Response(
            JSON.stringify({
              configured: true,
              source: "keyring",
              api_key: "must-not-cross-the-response-boundary",
            }),
            { status: 200, headers: { "Content-Type": "application/json" } }
          )
        )
    );

    await requestDesktopHostProductSurface();
    await expect(fetchProviderCredentialStatus("deepseek")).rejects.toThrow(
      "Provider credential status is invalid."
    );
    await expect(fetchProviderCredentialStatus("deepseek")).rejects.toThrow(
      "Provider credential status is invalid."
    );
  });
});

function ticket(character: string) {
  return {
    ticket: character.repeat(64),
    ttl_seconds: 30,
    http_base_url: HTTP_BASE_URL,
  };
}

function statusResponse(source: "keyring" | "missing") {
  return new Response(
    JSON.stringify({ configured: source !== "missing", source }),
    { status: 200, headers: { "Content-Type": "application/json" } }
  );
}
