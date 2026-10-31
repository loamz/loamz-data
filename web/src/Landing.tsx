import { Bot, ChevronRight, Database, FileText, History, Layers, ShieldCheck, Trash2, UsersRound } from "lucide-react";
import { useState } from "react";
import { Badge, Button } from "@/design";
import { hasOidc, startLogin } from "./auth";
import { Mark } from "./Mark";

/** What a visitor who is not signed in sees at the home address: what Loamz Data is, with a way to sign in. No account is needed to read it. */
export function Landing({ notice }: { notice?: string }) {
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState(notice ?? "");
  const signIn = () => {
    setBusy(true);
    startLogin().catch((e: Error) => {
      setProblem(e.message);
      setBusy(false);
    });
  };
  return (
    <div className="grid min-h-full grid-rows-[auto_1fr_auto] bg-surface text-foreground">
      <header className="mx-auto flex w-full max-w-6xl items-center gap-4 px-5 py-3">
        <a href="/" className="mr-auto inline-flex items-center gap-2.5 text-lg font-extrabold text-foreground" aria-label="Loamz Data home">
          <Mark size={26} /> Loamz Data
        </a>
        <nav aria-label="Main" className="hidden gap-6 text-sm sm:flex">
          <a href="#features" className="text-foreground hover:underline">Features</a>
          <a href="#ai" className="text-foreground hover:underline">AI</a>
          <a href="#next" className="text-foreground hover:underline">On the way</a>
        </nav>
        <Button variant="secondary" size="lg" onClick={signIn} disabled={busy || !hasOidc}>
          Sign in
        </Button>
      </header>

      <main className="mx-auto grid w-full max-w-6xl gap-16 px-5 pb-12">
        <section className="grid items-center gap-10 pt-8 md:grid-cols-[1.2fr_1fr]">
          <div className="grid justify-items-start gap-5">
            <Badge tone="accent" className="px-3 py-1 text-xs">Private beta</Badge>
            <h1 className="text-[clamp(2.5rem,7vw,4.25rem)] font-bold leading-[1.02] tracking-tight">Your data, and who can reach it.</h1>
            <p className="max-w-xl text-lg text-muted">
              Loamz Data keeps your files and tables in one governed place. Everyone gets exactly the access they were given, every change is recorded, and when you erase something, it is really gone.
            </p>
            <div className="flex flex-wrap items-center gap-3">
              <Button variant="primary" size="lg" className="px-6" onClick={signIn} disabled={busy || !hasOidc}>
                Sign in
              </Button>
              <a href="#features" className="text-sm font-medium text-foreground hover:underline">See how it works</a>
            </div>
            {!hasOidc && <p className="text-xs text-danger">Sign-in is not set up for this address: the app was built without VITE_OIDC_AUTHORITY.</p>}
            {problem && <p role="alert" className="text-xs text-danger">{problem}</p>}
          </div>

          <div className="justify-self-end w-full max-w-md rounded-2xl border border-border bg-card p-5 shadow-md" aria-hidden="true">
            <div className="space-y-1 text-sm">
              <Row icon={Database} label="sales" depth={0} />
              <Row icon={Layers} label="raw" depth={1} />
              <Row icon={FileText} label="uploads" depth={2} tag={<Badge tone="accent">/finance/analysts · Reader</Badge>} />
            </div>
            <div className="mt-4 space-y-2 border-t border-border pt-4 text-xs">
              <p className="flex items-center gap-2 text-foreground"><History className="text-muted" /> <span><b>ana</b> gave <b>/finance/analysts</b> read access</span></p>
              <p className="flex items-center gap-2 text-foreground"><History className="text-muted" /> <span><b>cyd</b> read <b>orders.csv</b></span></p>
              <p className="flex items-center gap-2 text-foreground"><History className="text-muted" /> <span><b>cyd</b> tried to write to <b>uploads</b></span> <Badge tone="danger">Refused</Badge></p>
            </div>
          </div>
        </section>

        <section id="features" aria-labelledby="features-title">
          <h2 id="features-title" className="mb-5 text-2xl font-semibold">Built to be trusted</h2>
          <ul className="grid gap-4 [grid-template-columns:repeat(auto-fit,minmax(15rem,1fr))]">
            <Feature icon={ShieldCheck} title="Governed by default">
              Catalogs, schemas and volumes with the privileges you know from Unity Catalog. Access goes to people, groups and services, and you see only what you were given.
            </Feature>
            <Feature icon={History} title="A record you can check">
              Every change, and every refusal, goes into a tamper-evident log that each organization keeps for itself. Check it whenever you like.
            </Feature>
            <Feature icon={Trash2} title="Erase means erase">
              Removing a person deletes what they owned and the files inside, takes their access away, and checks that nothing is left. The record of what happened stays.
            </Feature>
            <Feature icon={UsersRound} title="Made for teams">
              One sign-in with the rest of Loamz. An organization has workspaces, each with its own members, and decides which catalogs show where.
            </Feature>
          </ul>
        </section>

        <section id="ai" aria-labelledby="ai-title" className="grid items-center gap-8 md:grid-cols-[1fr_1.1fr]">
          <div className="grid justify-items-start gap-3">
            <Badge tone="neutral">On the way</Badge>
            <h2 id="ai-title" className="text-2xl font-semibold">Ask your data in plain words, with the same rules</h2>
            <p className="max-w-xl text-muted">
              An AI agent can query your warehouse for you, and it acts as you: it sees only the tables you were given, every question it runs is recorded next to yours, and what it may not read is simply not there. We are building it step by step; this is where it is going.
            </p>
          </div>
          <div className="w-full rounded-2xl border border-border bg-card p-5 shadow-md" aria-hidden="true">
            <p className="flex items-start gap-2 text-sm text-foreground">
              <Bot className="mt-0.5 shrink-0 text-accent" /> <span>Which region sold the most last month?</span>
            </p>
            <pre className="mt-3 overflow-x-auto rounded-md border border-border bg-background/50 p-3 font-mono text-xs text-foreground">{"SELECT region, sum(amount) AS total\nFROM sales.raw.orders\nGROUP BY region ORDER BY total DESC"}</pre>
            <div className="mt-3 space-y-2 border-t border-border pt-3 text-xs">
              <p className="flex items-center gap-2 text-foreground"><ShieldCheck className="text-muted" /> <span>Ran as <b>ana</b>'s agent: only what <b>ana</b> may read</span> <Badge tone="success">Allowed</Badge></p>
              <p className="flex items-center gap-2 text-foreground"><ShieldCheck className="text-muted" /> <span>Tried <b>hr.payroll.salaries</b>: not there for this agent</span> <Badge tone="danger">Refused</Badge></p>
              <p className="flex items-center gap-2 text-foreground"><History className="text-muted" /> <span>Both are in the record, with who asked</span></p>
            </div>
          </div>
        </section>

        <section id="how-ai" aria-labelledby="how-ai-title" className="-mt-6 grid gap-6">
          <div className="flex flex-wrap items-center gap-3">
            <h3 id="how-ai-title" className="text-lg font-semibold">How it will look in code</h3>
            <Badge tone="neutral">Illustrative: the API is not final</Badge>
          </div>
          <div className="grid items-start gap-6 md:grid-cols-[1.1fr_1fr]">
            <pre className="scroll-area overflow-x-auto rounded-2xl border border-border bg-card p-5 font-mono text-xs leading-relaxed text-foreground shadow-md" aria-label="Example code">
              <code>
                <span className="text-muted"># Give the code interpreter the warehouse,</span>{"\n"}
                <span className="text-muted"># and the signed-in person's credentials.</span>{"\n"}
                code = CodeInterpreter({"\n"}
                {"    "}loamz_data=<span className="text-accent">True</span>,{"\n"}
                {"    "}credentials=session.token,{"\n"}
                ){"\n\n"}
                agent = Agent(model=<span className="text-accent">"claude-sonnet-5-5"</span>, tools=[code]){"\n"}
                answer = <span className="text-accent">await</span> agent.run({"\n"}
                {"    "}<span className="text-accent">"Which region sold the most last month?"</span>{"\n"}
                )
              </code>
            </pre>
            <ol className="grid gap-2" aria-label="What happens, in order">
              <Step n={1} title="Your app" note="The person is signed in. You hand their credentials to the code interpreter." />
              <Step n={2} title="The code interpreter" note="Runs the agent's code in a sandbox with no network and no token. It can only ask the host for data." />
              <Step n={3} title="Loamz Data" note="The host asks as that person. Loamz Data takes what the person and the agent both allow, and records it." />
              <Step n={4} title="The warehouse" note="Runs it on compute that holds credentials for those tables only, for minutes." last />
            </ol>
          </div>
        </section>

        <section id="next" aria-labelledby="next-title">
          <h2 id="next-title" className="mb-3 text-2xl font-semibold">On the way</h2>
          <p className="max-w-3xl text-muted">
            A SQL editor and serverless warehouses for ad-hoc queries, job compute for daily pipelines, and isolated sandboxes where an agent can analyse data without ever touching your storage. They run on compute you attach, close to the data, and use the same access rules as everything else.
          </p>
        </section>
      </main>

      <footer className="mx-auto grid w-full max-w-6xl gap-2 border-t border-border px-5 pb-8 pt-6 text-sm text-muted">
        <p>
          Run by Ravikumar Chavva, Bengaluru, India. Support: <a className="text-accent hover:underline" href="mailto:support@loamz.com">support@loamz.com</a> · Privacy: <a className="text-accent hover:underline" href="mailto:privacy@loamz.com">privacy@loamz.com</a>
        </p>
      </footer>
    </div>
  );
}

function Step({ n, title, note, last }: { n: number; title: string; note: string; last?: boolean }) {
  return (
    <li className="flex items-start gap-3 rounded-xl border border-border bg-card px-4 py-3">
      <span className="mt-0.5 grid size-6 shrink-0 place-items-center rounded-full bg-accent/15 text-xs font-semibold text-accent">{n}</span>
      <span className="min-w-0 flex-1">
        <span className="block text-sm font-medium text-foreground">{title}</span>
        <span className="block text-xs text-muted">{note}</span>
      </span>
      {!last && <ChevronRight className="mt-1 shrink-0 rotate-90 text-muted" aria-hidden />}
    </li>
  );
}

function Row({ icon: Icon, label, depth, tag }: { icon: typeof Database; label: string; depth: number; tag?: React.ReactNode }) {
  return (
    <div className="flex items-center gap-2" style={{ paddingLeft: depth * 18 }}>
      <Icon className="text-muted" />
      <span className="font-medium text-foreground">{label}</span>
      {tag}
    </div>
  );
}

function Feature({ icon: Icon, title, children }: { icon: typeof Database; title: string; children: React.ReactNode }) {
  return (
    <li className="grid content-start gap-2 rounded-2xl border border-border bg-card p-5">
      <Icon className="icon-free size-6 text-accent" aria-hidden />
      <h3 className="mt-1 text-base font-semibold">{title}</h3>
      <p className="text-sm text-muted">{children}</p>
    </li>
  );
}
