import AccountDeletionSurface from "./views/components/AccountDeletionSurface";
import { consumeSecureMemberEntry } from "./lib/central/secureMemberEntry";
import { selectRemoteMember } from "./lib/remote/remoteWorkspace";
import { isMemberLoginPopup } from "./lib/central/memberPopup";
import MemberLoginPopup from "./views/components/MemberLoginPopup";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import StartupIdentityBoundary from "./views/components/StartupIdentityBoundary";
import LocalAttendeePanel from "./views/components/LocalAttendeePanel";
import ProviderSetupPanel from "./views/components/ProviderSetupPanel";
import { isBundledDesktopWebview, isDesktopWebview } from "./lib/desktopBridge";
import "./index.css";
import { clearCentralMemberRequest, consumeMemberReturn, consumeCentralMemberRequest, type MemberTargetRequest } from "./lib/central/memberConnect";
import { isCentralWebEntry } from "./lib/central/identity";
import MemberJoinPanel from "./views/components/MemberJoinPanel";

const memberLoginPopup = isMemberLoginPopup();
const memberReturn = consumeMemberReturn();
let memberRequest: MemberTargetRequest | undefined;
let memberEntryError = "";
if (isCentralWebEntry()) {
  try {
    const secureEntry = consumeSecureMemberEntry();
    if (secureEntry) selectRemoteMember(secureEntry);
    else if (!memberLoginPopup) memberRequest = consumeCentralMemberRequest();
    if (!secureEntry && !memberRequest && window.location.pathname === "/member-join") memberEntryError = "입장 요청이 없어요. 원래 초대 링크를 다시 열어 주세요.";
  } catch {
    memberEntryError = "입장 요청이 만료됐거나 올바르지 않아요. 원래 초대 링크에서 다시 시도해 주세요.";
    try { clearCentralMemberRequest(); }
    catch { memberEntryError = "브라우저 저장소를 사용할 수 없어요. 저장소 접근을 허용한 뒤 다시 시도해 주세요."; }
  }
}

const localAttendee = isBundledDesktopWebview() && new URL(window.location.href).searchParams.has("attendee-create");
const setupProvider = isDesktopWebview()
  ? new URL(window.location.href).searchParams.get("provider-setup") : null;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <AccountDeletionSurface>
    {memberLoginPopup ? <MemberLoginPopup /> : localAttendee ? <LocalAttendeePanel /> : memberRequest || memberEntryError ? <MemberJoinPanel request={memberRequest} entryError={memberEntryError} /> : <StartupIdentityBoundary memberReturn={memberReturn}>
      {({ deviceToken, clientId }) => (
        setupProvider ? <ProviderSetupPanel providerId={setupProvider} />
          : <App deviceToken={deviceToken} clientId={clientId} memberReturn={memberReturn} />
      )}
    </StartupIdentityBoundary>}
    </AccountDeletionSurface>
  </React.StrictMode>
);
