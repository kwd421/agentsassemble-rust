import "./styles/componentOrder";
import AppView from "./app/AppView";
import { useAppController } from "./app/useAppController";

export default function App({
  deviceToken,
  clientId,
}: {
  deviceToken: string;
  clientId: string;
}) {
  const controller = useAppController(deviceToken, clientId);
  return <AppView controller={controller} />;
}
