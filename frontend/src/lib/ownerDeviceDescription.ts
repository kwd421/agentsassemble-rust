import Bowser from "bowser";
import type { OwnerDeviceDescription } from "../types/generated/OwnerDeviceDescription";

export function browserDeviceDescription(): OwnerDeviceDescription {
  const parser = Bowser.getParser(navigator.userAgent);
  const browser = parser.getBrowserName();
  const os = parser.getOSName();
  return { device_name: [browser, os].filter(Boolean).join(" · "), browser, os };
}
