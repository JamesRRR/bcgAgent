import AppShell from "@/components/AppShell";
import UpdaterBanner from "@/components/UpdaterBanner";
import { useApp } from "@/state";
import Library from "@/pages/Library";
import Import from "@/pages/Import";
import Handbook from "@/pages/Handbook";
import Walkthrough from "@/pages/Walkthrough";
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
    case "walkthrough":
      return <Walkthrough />;
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
      <UpdaterBanner />
    </AppShell>
  );
}
