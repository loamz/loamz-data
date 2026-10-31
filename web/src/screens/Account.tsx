import { CheckCircle2, Copy, Cpu, KeyRound, Layers, Plus, ScrollText, ShieldAlert, ShieldCheck, Trash2, UserMinus, UserX, UsersRound } from "lucide-react";
import { useEffect, useState } from "react";
import { Badge, Button, Dialog, DialogContent, DialogFooter, Input, NavItem, Page, PageEmpty, Section, Segmented, Select, confirmAction, toast } from "@/design";
import * as api from "@/api";
import { Principal } from "@/components/Principal";
import { PrincipalField } from "@/components/PrincipalField";
import { describe, shortTarget, when } from "@/format";
import { useLoad } from "@/hooks";
import { useNames } from "@/names";
import { openWorkspace } from "@/site";
import { go, type AccountTab } from "@/route";
import { centralOrigin } from "@/site";

const tabs: { id: AccountTab; label: string; icon: typeof Layers }[] = [
  { id: "workspaces", label: "Workspaces", icon: Layers },
  { id: "compute", label: "Compute", icon: Cpu },
  { id: "admins", label: "Account admins", icon: UsersRound },
  { id: "activity", label: "Activity", icon: ScrollText },
  { id: "erase", label: "Erase a person", icon: UserX },
];

/** The account console of an organization, for its account admins: workspaces, who manages the account, the audit trail, erasing a person. */
export function Account({ org, tab, me }: { org: string; tab: AccountTab; me: api.Me | undefined }) {
  const info = me?.orgs.find((o) => o.id === org);
  if (me && !info) {
    return (
      <Page title="Account">
        <PageEmpty icon={ShieldAlert} title="This organization isn't here, or you don't belong to it">
          Check the address, or go back Home.
        </PageEmpty>
      </Page>
    );
  }
  if (!info) return <Page title="Account">{null}</Page>;
  if (!info.has_account || !info.account_admin) {
    return (
      <Page title={info.alias} subtitle="Account console">
        <PageEmpty icon={ShieldAlert} title={info.has_account ? "Only account admins can manage this" : "This organization has no Data account yet"}>
          {info.has_account ? "Ask an account admin to add you." : info.can_create_account ? "You can create it from Home." : "Someone with the account-admin role in Keycloak creates it."}
        </PageEmpty>
      </Page>
    );
  }
  return (
    <div className="flex h-full min-h-0 flex-col md:flex-row">
      <aside className="flex shrink-0 gap-1 overflow-x-auto border-b border-border bg-sidebar p-3 md:w-64 md:flex-col md:overflow-visible md:border-b-0 md:border-r md:p-4">
        <div className="hidden px-3 pb-2 md:block">
          <p className="truncate text-sm font-semibold text-foreground">{info.alias}</p>
          <p className="text-xs text-muted">Account console</p>
        </div>
        {tabs.map(({ id, label, icon }) => (
          <NavItem key={id} icon={icon} active={tab === id} onClick={() => go({ screen: "account", org, tab: id })} className="h-control-md w-auto md:w-full">
            {label}
          </NavItem>
        ))}
      </aside>
      <section className="min-h-0 min-w-0 flex-1">
        {tab === "workspaces" && <Workspaces org={org} />}
        {tab === "compute" && <ComputeTab org={org} />}
        {tab === "admins" && <Admins org={org} />}
        {tab === "activity" && <Activity org={org} ws={me?.workspaces.find((w) => w.org_id === org)?.id} />}
        {tab === "erase" && <Erase org={org} />}
      </section>
    </div>
  );
}

function Workspaces({ org }: { org: string }) {
  const list = useLoad(() => api.workspaces(org), org);
  const [name, setName] = useState("");
  return (
    <Page title="Workspaces" subtitle="Where people work. Create one per team, and assign who may open it.">
      <form
        className="flex items-center gap-2"
        onSubmit={async (e) => {
          e.preventDefault();
          try {
            const w = await api.createWorkspace(org, name.trim());
            toast.success(`Created ${w.name}`);
            setName("");
            list.reload();
          } catch (err) {
            toast.error("Could not create it", (err as Error).message);
          }
        }}
      >
        <Input className="max-w-xs" placeholder="New workspace name" aria-label="Workspace name" value={name} onChange={(e) => setName(e.target.value)} />
        <Button variant="primary" type="submit" disabled={!name.trim()}>
          Create
        </Button>
      </form>
      <div className="divide-y divide-border rounded-xl border border-border bg-background/40">
        {(list.data ?? []).map((w) => (
          <div key={w.id} className="flex items-center gap-3 px-4 py-3">
            <Layers className="shrink-0 text-muted" aria-hidden />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-sm font-medium text-foreground">{w.name}</span>
              <span className="block text-xs text-muted">Created {when(w.created_ms)}</span>
            </span>
            <Button variant="secondary" onClick={() => openWorkspace(w.id, "/settings")}>
              Settings
            </Button>
            <Button variant="secondary" onClick={() => openWorkspace(w.id)}>
              Open
            </Button>
          </div>
        ))}
      </div>
    </Page>
  );
}

function Admins({ org }: { org: string }) {
  const list = useLoad(() => api.admins(org), org);
  const [principal, setPrincipal] = useState("");
  return (
    <Page title="Account admins" subtitle="They manage workspaces and who is in them, every catalog, and the audit trail. An account always keeps at least one.">
      <form
        className="flex flex-wrap items-center gap-2"
        onSubmit={async (e) => {
          e.preventDefault();
          try {
            await api.addAdmin(org, principal);
            toast.success("Added");
            list.reload();
          } catch (err) {
            toast.error("Could not add", (err as Error).message);
          }
        }}
      >
        <PrincipalField onChange={setPrincipal} className="min-w-72 max-w-2xl" />
        <Button variant="primary" type="submit" disabled={!principal}>
          Make admin
        </Button>
      </form>
      <div className="divide-y divide-border rounded-xl border border-border bg-background/40">
        {(list.data ?? []).map((p) => (
          <div key={p} className="flex items-center gap-3 px-4 py-2.5">
            <span className="min-w-0 flex-1">
              <Principal id={p} />
            </span>
            <Button
              variant="danger"
              size="icon-sm"
              aria-label={`Remove ${p} as admin`}
              onClick={async () => {
                if (!(await confirmAction({ title: "Remove as account admin?", confirmLabel: "Remove", danger: true }))) return;
                try {
                  await api.removeAdmin(org, p);
                  list.reload();
                } catch (err) {
                  toast.error("Could not remove", (err as Error).message);
                }
              }}
            >
              <UserMinus />
            </Button>
          </div>
        ))}
      </div>
    </Page>
  );
}

function Activity({ org, ws }: { org: string; ws: string | undefined }) {
  const trail = useLoad(() => api.audit(org, 200), org);
  const [only, setOnly] = useState<"all" | "denied">("all");
  const [verdict, setVerdict] = useState<api.Verdict | null>(null);
  const [checking, setChecking] = useState(false);
  const all = trail.data ?? [];
  const names = useNames(ws, all.map((e) => e.target));
  const shown = only === "denied" ? all.filter((e) => e.outcome === "denied") : all;

  return (
    <Page
      title="Activity"
      subtitle="Everything done in this organization, newest first. Entries can't be changed or removed, and each organization has its own record."
      actions={
        <>
          {verdict && (
            <Badge tone={verdict.intact ? "success" : "danger"}>
              {verdict.intact ? <ShieldCheck className="size-3" /> : <ShieldAlert className="size-3" />}
              {verdict.intact ? `Intact · ${verdict.entries} entries` : `Broken at entry ${verdict.first_broken}`}
            </Badge>
          )}
          <Button
            disabled={checking}
            onClick={async () => {
              setChecking(true);
              try {
                setVerdict(await api.verify(org));
              } catch (e) {
                toast.error("Could not check the record", (e as Error).message);
              } finally {
                setChecking(false);
              }
            }}
          >
            <CheckCircle2 /> {checking ? "Checking…" : "Check the record"}
          </Button>
        </>
      }
    >
      <div className="flex items-center justify-between gap-2">
        <p className="text-xs text-muted">{trail.loading && !trail.data ? "Loading…" : `Showing the latest ${all.length}.`}</p>
        <Segmented label="Show" value={only} onChange={setOnly} options={[{ value: "all", label: "All" }, { value: "denied", label: "Refused" }]} />
      </div>
      {shown.length === 0 && !trail.loading ? (
        <PageEmpty icon={ScrollText} title={only === "denied" ? "Nothing was refused" : "No activity yet"}>
          {only === "denied" ? "Every request in this list was allowed." : "Things appear here as soon as someone does something."}
        </PageEmpty>
      ) : (
        <div className="divide-y divide-border rounded-xl border border-border bg-background/40">
          {shown.map((e) => (
            <div key={e.seq} className="flex items-center gap-3 px-4 py-2.5">
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm text-foreground">
                  <span className="font-medium" title={e.actor}>{e.actor.length > 20 ? `${e.actor.slice(0, 8)}…` : e.actor}</span>
                  {e.on_behalf_of && <span className="text-muted"> for <span title={e.on_behalf_of}>{e.on_behalf_of.slice(0, 8)}…</span></span>}
                  <span className="text-muted"> · </span>
                  {describe(e.action, e.outcome)}
                </p>
                <p className="truncate text-xs text-muted" title={e.target}>{shortTarget(e.target, names)}</p>
              </div>
              {e.outcome !== "ok" && <Badge tone="danger">{e.outcome === "denied" ? "Refused" : e.outcome}</Badge>}
              <span className="w-20 shrink-0 text-right text-xs tabular-nums text-muted">{when(e.at_ms)}</span>
            </div>
          ))}
        </div>
      )}
    </Page>
  );
}

function Erase({ org }: { org: string }) {
  const [subject, setSubject] = useState("");
  const [taskId, setTaskId] = useState("");
  const [task, setTask] = useState<api.Task | null>(null);

  // Follow the task until it settles: an erase runs in the background and checks itself.
  useEffect(() => {
    if (!taskId) return;
    let stop = false;
    const tick = async () => {
      try {
        const t = await api.task(org, taskId);
        if (stop) return;
        setTask(t);
        if (t.state === "pending") setTimeout(tick, 1500);
      } catch {
        /* the next click will show the problem */
      }
    };
    void tick();
    return () => {
      stop = true;
    };
  }, [org, taskId]);

  return (
    <Page title="Erase a person" subtitle="Removes everything they own here, the files inside it, and every access they hold. The record of what happened is kept.">
      <Section title="Who">
        <form
          className="flex flex-wrap items-center gap-2"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!(await confirmAction({ title: "Erase this person's data?", description: "Their catalogs, volumes, files, access and assignments are removed for good. This can't be undone.", confirmLabel: "Erase", danger: true }))) return;
            try {
              const r = await api.erase(org, subject);
              setTask(null);
              setTaskId(r.task_id);
              toast.info("Erase started");
            } catch (err) {
              toast.error("Could not start the erase", (err as Error).message);
            }
          }}
        >
          <PrincipalField onChange={setSubject} className="min-w-72 max-w-2xl" />
          <Button variant="danger" type="submit" disabled={!subject}>
            <UserX /> Erase
          </Button>
        </form>
        {taskId && (
          <p className="mt-4 flex items-center gap-2 text-sm text-foreground">
            <Badge tone={task?.state === "done" ? "success" : task?.state === "failed" ? "danger" : "neutral"}>{task?.state ?? "starting"}</Badge>
            <span className="text-xs text-muted">
              {task?.state === "failed" ? task.last_error || "It gave up after several tries." : task?.state === "done" ? "Everything was removed and checked." : task?.last_error || "Working. It keeps going even if the server restarts."}
            </span>
          </p>
        )}
      </Section>
    </Page>
  );
}

const MEMORY = [
  { value: "1024", label: "1 GB" },
  { value: "2048", label: "2 GB" },
  { value: "4096", label: "4 GB" },
  { value: "8192", label: "8 GB" },
  { value: "16384", label: "16 GB" },
];
const CPU = [
  { value: "500", label: "½ CPU" },
  { value: "1000", label: "1 CPU" },
  { value: "2000", label: "2 CPUs" },
  { value: "4000", label: "4 CPUs" },
];

/** Compute an organization has attached. A machine connects out to this server and asks for work, so nothing is opened on it. */
function ComputeTab({ org }: { org: string }) {
  const list = useLoad(() => api.attachedCompute(org), org);
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [memory, setMemory] = useState("2048");
  const [cpu, setCpu] = useState("1000");
  const [shown, setShown] = useState<api.Compute | null>(null);
  const [kind, setKind] = useState<"agent" | "serverless">("agent");
  const [endpoint, setEndpoint] = useState("");
  const items = list.data ?? [];

  // Anyone watching the list sees a machine come online as soon as its agent connects.
  const waiting = items.some((c) => !c.online && c.kind === "agent");
  useEffect(() => {
    if (!waiting) return;
    const t = setInterval(list.reload, 5000);
    return () => clearInterval(t);
  }, [waiting, list.reload]);

  return (
    <Page
      title="Compute"
      subtitle="The machines your workspaces run statements and jobs on. Each connects out to this server, so nothing is opened on it."
      actions={
        <Button variant="primary" onClick={() => setAdding(true)}>
          <Plus /> Attach compute
        </Button>
      }
    >
      {items.length === 0 && !list.loading ? (
        <PageEmpty icon={Cpu} title="No compute attached" action={<Button variant="primary" onClick={() => setAdding(true)}><Plus /> Attach compute</Button>}>
          Attach a machine, then start the agent on it with the secret you are shown. Until then nothing can run.
        </PageEmpty>
      ) : (
        <div className="divide-y divide-border rounded-xl border border-border bg-background/40">
          {items.map((c) => (
            <div key={c.id} className="flex flex-wrap items-center gap-3 px-4 py-3">
              <Cpu className="shrink-0 text-muted" aria-hidden />
              <span className="min-w-0 flex-1 basis-48">
                <span className="block truncate text-sm font-medium text-foreground">{c.name}</span>
                <span className="block text-xs text-muted">Up to {c.max_memory_mb.toLocaleString()} MB and {c.max_cpu_millis / 1000} CPU per run{c.kind === "serverless" ? " · starts when a statement arrives, stops when idle" : c.last_seen_ms ? ` · last heard ${when(c.last_seen_ms)}` : " · its agent has not connected yet"}</span>
              </span>
              <Badge tone="neutral">{c.kind === "serverless" ? "serverless" : "machine"}</Badge>
              <Badge tone={c.online ? "success" : "neutral"}>{c.online ? (c.kind === "serverless" ? "ready" : "online") : "offline"}</Badge>
              {c.kind === "agent" && (
              <Button
                variant="secondary"
                size="sm"
                onClick={async () => {
                  if (!(await confirmAction({ title: `Give ${c.name} a new secret?`, description: "The agent using the old one is turned away until it is given the new one.", confirmLabel: "New secret" }))) return;
                  try {
                    setShown(await api.newComputeSecret(org, c.id));
                    list.reload();
                  } catch (e) {
                    toast.error("Could not make a new secret", (e as Error).message);
                  }
                }}
              >
                <KeyRound /> New secret
              </Button>
              )}
              <Button
                variant="danger"
                size="icon-sm"
                aria-label={`Remove ${c.name}`}
                onClick={async () => {
                  if (!(await confirmAction({ title: `Remove ${c.name}?`, description: "What is waiting or running on it fails.", confirmLabel: "Remove", danger: true }))) return;
                  try {
                    await api.detachCompute(org, c.id);
                    list.reload();
                  } catch (e) {
                    toast.error("Could not remove it", (e as Error).message);
                  }
                }}
              >
                <Trash2 />
              </Button>
            </div>
          ))}
        </div>
      )}

      <Dialog open={adding} onOpenChange={setAdding}>
        <DialogContent title="Attach compute" description="A machine of yours that connects out, or a serverless endpoint that starts only when a statement arrives.">
          <form
            className="mt-4 space-y-4"
            onSubmit={async (e) => {
              e.preventDefault();
              try {
                const made = await api.attachCompute(org, { name: name.trim(), kind, endpoint: kind === "serverless" ? endpoint.trim() : undefined, max_memory_mb: Number(memory), max_cpu_millis: Number(cpu) });
                setAdding(false);
                setName("");
                setEndpoint("");
                if (made.secret) setShown(made);
                list.reload();
              } catch (err) {
                toast.error("Could not attach it", (err as Error).message);
              }
            }}
          >
            <label className="block space-y-1.5">
              <span className="text-xs font-medium text-foreground">Kind</span>
              <Select aria-label="Kind" value={kind} onValueChange={(v) => setKind(v as "agent" | "serverless")} options={[{ value: "agent", label: "A machine of yours (it connects out)" }, { value: "serverless", label: "Serverless (started on demand)" }]} />
            </label>
            <label className="block space-y-1.5">
              <span className="text-xs font-medium text-foreground">Name</span>
              <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder={kind === "agent" ? "oracle" : "modal"} />
            </label>
            {kind === "serverless" && (
              <label className="block space-y-1.5">
                <span className="text-xs font-medium text-foreground">Endpoint</span>
                <Input value={endpoint} onChange={(e) => setEndpoint(e.target.value)} placeholder="https://your-app.modal.run" spellCheck={false} />
                <span className="block text-2xs text-muted">Only statements over data classified normal run here. It is given credentials for the tables of one statement, for minutes.</span>
              </label>
            )}
            <div className="grid grid-cols-2 gap-3">
              <label className="block space-y-1.5">
                <span className="text-xs font-medium text-foreground">Memory per run</span>
                <Select aria-label="Memory per run" value={memory} onValueChange={setMemory} options={MEMORY} />
              </label>
              <label className="block space-y-1.5">
                <span className="text-xs font-medium text-foreground">CPU per run</span>
                <Select aria-label="CPU per run" value={cpu} onValueChange={setCpu} options={CPU} />
              </label>
            </div>
            <DialogFooter>
              <Button type="submit" variant="primary" disabled={!name.trim() || (kind === "serverless" && !endpoint.trim().startsWith("https://"))}>
                Attach
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog open={shown !== null} onOpenChange={(o) => !o && setShown(null)}>
        <DialogContent title={`Start the agent on ${shown?.name ?? ""}`} description="This secret is shown once. Keep it in a file only the agent's user can read." className="max-w-xl">
          {shown?.secret && <Secret compute={shown} />}
        </DialogContent>
      </Dialog>
    </Page>
  );
}

function Secret({ compute }: { compute: api.Compute }) {
  const secret = compute.secret ?? "";
  const command = `umask 077 && printf '%s' '${secret}' > ~/.loamz-agent.secret

LOAMZ_DATA_URL=${centralOrigin} \\
LOAMZ_AGENT_SECRET_FILE=~/.loamz-agent.secret \\
LOAMZ_RUNSC=~/.cache/loamz/runsc \\
LOAMZ_ROOTFS=~/.cache/loamz/rootfs \\
loamz-runner agent`;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command);
      toast.success("Copied");
    } catch {
      toast.error("Could not copy", "Select the text and copy it.");
    }
  };
  return (
    <div className="mt-4 space-y-3">
      <p className="text-xs text-muted">On the machine, in a shell (it needs <span className="font-mono">runsc</span> and the sandbox filesystem; <span className="font-mono">scripts/fetch-runsc.sh</span> and <span className="font-mono">server/runner/rootfs/build.sh</span> make them):</p>
      <pre className="scroll-area whitespace-pre-wrap break-all rounded-md border border-border bg-background/50 p-3 font-mono text-xs text-foreground">{command}</pre>
      <div className="flex justify-end">
        <Button variant="secondary" onClick={() => void copy()}>
          <Copy /> Copy
        </Button>
      </div>
      <p className="text-xs text-muted">When the agent connects, the machine shows as online here within a few seconds.</p>
    </div>
  );
}
