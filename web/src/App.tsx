import { useEffect, useState, useSyncExternalStore } from "react";
import { ConfirmHost, Toaster, TooltipProvider } from "@/design";
import { AccountShell } from "./AccountShell";
import { Entering } from "./Entering";
import { finishLogin, onAuthChange, signedIn } from "./auth";
import { Landing } from "./Landing";
import { site } from "./site";
import { WorkspaceShell } from "./WorkspaceShell";

const snapshot = () => (signedIn() ? "1" : "");

export function App() {
  const here = useSyncExternalStore(onAuthChange, snapshot) !== "";
  const [problem, setProblem] = useState("");
  const [ready, setReady] = useState(false);
  useEffect(() => {
    finishLogin()
      .catch((e: Error) => setProblem(e.message))
      .finally(() => setReady(true));
  }, []);
  if (!ready) return null;
  return (
    <TooltipProvider delayDuration={300}>
      {here ? site.kind === "workspace" ? <WorkspaceShell ws={site.ws} /> : <AccountShell /> : site.kind === "workspace" ? <Entering notice={problem} /> : <Landing notice={problem} />}
      <Toaster />
      <ConfirmHost />
    </TooltipProvider>
  );
}
