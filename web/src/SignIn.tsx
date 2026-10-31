import { useState } from "react";
import { Button, Input, Text } from "@/design";
import { hasOidc, setToken, startLogin } from "./auth";
import { Mark } from "./Mark";

export function SignIn({ notice }: { notice?: string }) {
  const [token, setPasted] = useState("");
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
        {hasOidc ? (
          <Button
            variant="primary"
            size="lg"
            className="w-full"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              startLogin().catch((e: Error) => {
                setProblem(e.message);
                setBusy(false);
              });
            }}
          >
            Continue to sign in
          </Button>
        ) : (
          <form
            className="space-y-3"
            onSubmit={(e) => {
              e.preventDefault();
              if (token.trim()) setToken(token);
            }}
          >
            <Text className="text-xs text-muted">Sign-in is not set up for this address yet. For development, paste a token from the dev server.</Text>
            <Input size="lg" placeholder="Paste a token" value={token} onChange={(e) => setPasted(e.target.value)} autoComplete="off" spellCheck={false} aria-label="Token" />
            <Button variant="primary" size="lg" className="w-full" type="submit" disabled={!token.trim()}>
              Continue
            </Button>
          </form>
        )}
        {problem && (
          <p role="alert" className="text-xs text-danger">
            {problem}
          </p>
        )}
      </div>
    </div>
  );
}
