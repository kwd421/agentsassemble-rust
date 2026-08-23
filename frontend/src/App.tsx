import AppView from "./app/AppView";
import { useAppController } from "./app/useAppController";
import StartupIdentityGate from "./views/components/StartupIdentityGate";

export default function App() {
  const controller = useAppController();
  if (!controller.startupIdentityResolved) {
    return (
      <StartupIdentityGate
        deviceToken={controller.deviceToken}
        onComplete={controller.completeStartupIdentity}
      />
    );
  }
  return <AppView controller={controller} />;
}
