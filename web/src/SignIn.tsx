import { useState } from "react";
import { Button } from "@/design";
import { hasOidc, startLogin } from "./auth";
import { Mark } from "./Mark";

export function SignIn({ notice }: { notice?: string }) {
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState(notice ?? "");
  return (
    <div className="flex h-full items-center justify-center bg-background px-4">
      <div className="w-full max-w-sm space-y-5">
        <div className="flex items-center gap-3">
          <Mark />
          <div>
            <h1 className="text-lg font-semibold tracking-tight text-foreground">Loamz Data</h1>
            <p className="text-xs text-muted">Your data, and who can reach it.</p>
          </div>
        </div>
        <Button
          variant="primary"
          size="lg"
          className="w-full"
          disabled={busy || !hasOidc}
          onClick={() => {
            setBusy(true);
            startLogin().catch((e: Error) => {
              setProblem(e.message);
              setBusy(false);
            });
          }}
        >
          Sign in with Loamz
        </Button>
        {!hasOidc && <p className="text-xs text-danger">Sign-in is not set up for this address: the app was built without VITE_OIDC_AUTHORITY.</p>}
        {problem && (
          <p role="alert" className="text-xs text-danger">
            {problem}
          </p>
        )}
      </div>
    </div>
  );
}
