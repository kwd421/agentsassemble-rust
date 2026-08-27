import "./app/originalImportOrder";
import AppView from "./app/AppView";
import { useAppController } from "./app/useAppController";

export default function App() {
  const controller = useAppController();
  return <AppView controller={controller} />;
}
