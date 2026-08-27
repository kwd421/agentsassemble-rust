import "./app/originalImportOrder";
import AppView from "./app/AppView";
import { useAppController } from "./app/useAppController";

export default function App({ deviceToken }: { deviceToken: string }) {
  const controller = useAppController(deviceToken);
  return <AppView controller={controller} />;
}
