import AppShell from "@/components/AppShell";
import { useApp } from "@/state";
import Library from "@/pages/Library";
import Import from "@/pages/Import";
import Handbook from "@/pages/Handbook";
import Ask from "@/pages/Ask";
import Settings from "@/pages/Settings";

function PageSwitch() {
  const { page } = useApp();
  switch (page) {
    case "library":
      return <Library />;
    case "import":
      return <Import />;
    case "handbook":
      return <Handbook />;
    case "ask":
      return <Ask />;
    case "settings":
      return <Settings />;
    default:
      return <Library />;
  }
}

export default function App() {
  return (
    <AppShell>
      <PageSwitch />
    </AppShell>
  );
}
