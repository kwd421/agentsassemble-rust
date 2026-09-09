import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import StartupIdentityBoundary from "./views/components/StartupIdentityBoundary";
import ProviderSetupPanel from "./views/components/ProviderSetupPanel";
import { isDesktopWebview } from "./lib/desktopBridge";
import "./index.css";

const setupProvider = isDesktopWebview()
  ? new URL(window.location.href).searchParams.get("provider-setup") : null;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <StartupIdentityBoundary>
      {({ deviceToken, clientId }) => (
        setupProvider ? <ProviderSetupPanel providerId={setupProvider} />
          : <App deviceToken={deviceToken} clientId={clientId} />
      )}
    </StartupIdentityBoundary>
  </React.StrictMode>
);
