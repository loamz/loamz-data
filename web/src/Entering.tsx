import { useEffect, useState } from "react";
import { Button } from "@/design";
import { leavingNow, startLogin } from "./auth";
import { Mark } from "./Mark";

/** A workspace site, signing you in. There is no front page here: if you already signed in to Loamz, it takes a moment and you are in. */
export function Entering({ notice }: { notice?: string }) {
  const [problem, setProblem] = useState(notice ?? "");
  useEffect(() => {
    if (problem || leavingNow()) return;
    startLogin().catch((e: Error) => setProblem(e.message));
  }, [problem]);
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 bg-background px-4 text-center">
      <Mark size={36} />
      {problem ? (
        <>
          <p role="alert" className="max-w-sm text-sm text-danger">{problem}</p>
          <Button variant="primary" onClick={() => setProblem("")}>
            Try again
          </Button>
        </>
      ) : (
        <p className="text-sm text-muted">Signing you in…</p>
      )}
    </div>
  );
}
