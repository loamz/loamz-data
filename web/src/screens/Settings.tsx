import { Building2, Bot, KeyRound, Trash2, UserMinus, Users, type LucideIcon } from "lucide-react";
import { useState, type ReactNode } from "react";
import { Badge, Body, Button, DataTable, Input, ObjectHeader, Page, PageEmpty, Select, SidebarItem, confirmAction, toast, type Column } from "@/design";
import * as api from "@/api";
import { Principal } from "@/components/Principal";
import { PrincipalField } from "@/components/PrincipalField";
import { ShownOnce } from "@/components/ShownOnce";
import { useLoad } from "@/hooks";
import { hrefOf, navigate, type Route, type SettingsTab } from "@/route";
import { centralOrigin } from "@/site";

const ROLES = [
  { value: "user", label: "User" },
  { value: "admin", label: "Admin" },
];

/** A workspace's members and what they are entitled to. For the workspace's admins. */
export function Settings({ ws, tab }: { ws: string; tab: SettingsTab; me: api.Me | undefined }) {
  const w = useLoad(() => api.workspace(ws), ws);
  const here = w.data;
  if (w.error) {
    return (
      <Page title="Workspace">
        <PageEmpty icon={Trash2} title="This workspace isn't here, or you are not part of it">{w.error.message}</PageEmpty>
      </Page>
    );
  }
  if (!here) return <Page title="Workspace settings">{null}</Page>;
  if (!here.admin) {
    return (
      <Body>
        <PageEmpty icon={UserMinus} title="Only admins can change this">
          Ask an admin of {here.name}.
        </PageEmpty>
      </Body>
    );
  }
  // Renaming and deleting belong to account admins; for anyone else that page is not offered, and its address shows the first one.
  const sections: { tab: SettingsTab; label: string; icon: LucideIcon }[] = [
    { tab: "members", label: "Members", icon: Users },
    { tab: "principals", label: "Service principals", icon: Bot },
    { tab: "entitlements", label: "Entitlements", icon: KeyRound },
    ...(here.account_admin ? [{ tab: "workspace" as const, label: "Workspace", icon: Building2 }] : []),
  ];
  const now = sections.find((s) => s.tab === tab)?.tab ?? "members";
  const to = (t: SettingsTab): Route => ({ screen: "settings", ws, tab: t });
  return (
    <Body>
      <ObjectHeader crumbs={[{ label: "Workspace settings" }]} title={here.name} />
      <div className="flex flex-col gap-3 md:flex-row md:gap-6">
        <nav aria-label="Settings" className="flex shrink-0 gap-0.5 overflow-x-auto md:w-44 md:flex-col">
          {sections.map((s) => (
            <SidebarItem key={s.tab} icon={s.icon} active={s.tab === now} href={hrefOf(to(s.tab))} onClick={navigate(to(s.tab))}>
              {s.label}
            </SidebarItem>
          ))}
        </nav>
        <div className="min-w-0 max-w-3xl flex-1">
          {now === "members" && <Members ws={ws} />}
          {now === "principals" && <ServicePrincipals ws={ws} />}
          {now === "entitlements" && <Entitlements ws={ws} />}
          {now === "workspace" && <Danger ws={ws} workspace={here} />}
        </div>
      </div>
    </Body>
  );
}

/** One page of the settings: its name and one line on what it does, then the form and the list. */
function Section({ title, note, children }: { title: string; note: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3">
      <div>
        <h2 className="text-base font-semibold">{title}</h2>
        <p className="text-sm text-muted">{note}</p>
      </div>
      {children}
    </section>
  );
}

function ServicePrincipals({ ws }: { ws: string }) {
  const list = useLoad(() => api.servicePrincipals(ws), ws);
  const [name, setName] = useState("");
  const [shown, setShown] = useState<{ name: string; token: string } | null>(null);
  const columns: Column<api.ServicePrincipal>[] = [
    { key: "name", header: "Name", width: "fill", cell: (p) => p.name },
    { key: "principal", header: "Principal", cell: (p) => <span className="font-mono text-xs text-muted">{p.principal}</span> },
    {
      key: "actions",
      header: "",
      cell: (p) => (
        <span className="flex gap-1">
          <Button
            variant="ghost"
            size="sm"
            onClick={async () => {
              if (!(await confirmAction({ title: `New token for ${p.name}?`, description: "The old token stops working at once.", confirmLabel: "Replace token" }))) return;
              try {
                setShown({ name: p.name, token: (await api.renewServicePrincipalToken(ws, p.id)).token });
              } catch (err) {
                toast.error("Could not renew it", (err as Error).message);
              }
            }}
          >
            New token
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={async () => {
              if (!(await confirmAction({ title: `Delete ${p.name}?`, description: "Its token stops working at once.", confirmLabel: "Delete" }))) return;
              try {
                await api.deleteServicePrincipal(ws, p.id);
                list.reload();
              } catch (err) {
                toast.error("Could not delete it", (err as Error).message);
              }
            }}
          >
            Delete
          </Button>
        </span>
      ),
    },
  ];
  return (
    <Section title="Service principals" note="An app or pipeline of this workspace (a BI tool, an ML app) that is not a person. It signs in with a token, works only here, and gets grants like anyone: name it as sp:<id>.">
      <form
        className="flex flex-wrap items-center gap-2"
        onSubmit={async (e) => {
          e.preventDefault();
          try {
            const made = await api.createServicePrincipal(ws, name.trim());
            setShown({ name: made.name, token: made.token });
            setName("");
            list.reload();
          } catch (err) {
            toast.error("Could not create it", (err as Error).message);
          }
        }}
      >
        <Input className="w-full sm:w-64" placeholder="Name, such as tableau" aria-label="Service principal name" value={name} onChange={(e) => setName(e.target.value)} />
        <Button variant="primary" type="submit" disabled={!name.trim()}>
          Create
        </Button>
      </form>
      <DataTable columns={columns} rows={list.data ?? []} rowKey={(p) => p.id} count={`${(list.data ?? []).length} service principals`} empty="No service principals yet." />
      <ShownOnce
        title={`Token for ${shown?.name ?? ""}`}
        description="It is shown once. Send it as a bearer token: Authorization: Bearer <token>."
        text={shown?.token ?? null}
        onClose={() => setShown(null)}
      />
    </Section>
  );
}

function Members({ ws }: { ws: string }) {
  const list = useLoad(() => api.members(ws), ws);
  const [principal, setPrincipal] = useState("");
  const [role, setRole] = useState("user");
  const columns: Column<api.Member>[] = [
    { key: "who", header: "Member", width: "fill", cell: (m) => <Principal id={m.principal} /> },
    {
      key: "role",
      header: "Role",
      cell: (m) => (
        <Select
          className="w-28"
          size="sm"
          aria-label={`Role of ${m.principal}`}
          value={m.role}
          onValueChange={async (r) => {
            try {
              await api.assignMember(ws, m.principal, r as "admin" | "user");
              list.reload();
            } catch (err) {
              toast.error("Could not change the role", (err as Error).message);
            }
          }}
          options={ROLES}
        />
      ),
    },
    {
      key: "remove",
      header: "",
      cell: (m) => (
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`Remove ${m.principal}`}
          onClick={async () => {
            if (!(await confirmAction({ title: "Remove from the workspace?", description: "They can no longer open it. Their access to data is unchanged.", confirmLabel: "Remove", danger: true }))) return;
            try {
              await api.removeMember(ws, m.principal);
              list.reload();
            } catch (err) {
              toast.error("Could not remove", (err as Error).message);
            }
          }}
        >
          <UserMinus />
        </Button>
      ),
    },
  ];
  return (
    <Section title="Members" note="People, groups and service principals assigned to this workspace. Admins manage it; users work in it.">
      <form
        className="flex flex-wrap items-center gap-2"
        onSubmit={async (e) => {
          e.preventDefault();
          if (!principal) return;
          try {
            await api.assignMember(ws, principal, role as "admin" | "user");
            toast.success("Assigned");
            list.reload();
          } catch (err) {
            toast.error("Could not assign", (err as Error).message);
          }
        }}
      >
        <PrincipalField onChange={setPrincipal} className="w-full sm:w-auto sm:min-w-80 sm:flex-1" />
        <Select className="w-28" aria-label="Role" value={role} onValueChange={setRole} options={ROLES} />
        <Button variant="primary" type="submit" disabled={!principal}>
          Assign
        </Button>
      </form>
      <DataTable columns={columns} rows={list.data ?? []} rowKey={(m) => m.principal} count={`${(list.data ?? []).length} assigned`} empty="Nobody is assigned yet." />
    </Section>
  );
}

function Entitlements({ ws }: { ws: string }) {
  const members = useLoad(() => api.members(ws), ws);
  const ent = useLoad(() => api.entitlements(ws), ws);
  const [who, setWho] = useState("");
  const [what, setWhat] = useState("");
  const available = ent.data?.available ?? [];
  const columns: Column<{ principal: string; entitlement: string }>[] = [
    { key: "who", header: "Member", width: "fill", cell: (e) => <Principal id={e.principal} /> },
    { key: "what", header: "Entitlement", cell: (e) => <Badge>{e.entitlement}</Badge> },
    {
      key: "take",
      header: "",
      cell: (e) => (
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`Take ${e.entitlement} from ${e.principal}`}
          onClick={async () => {
            await api.unentitle(ws, e.principal, e.entitlement).catch((err: Error) => toast.error("Could not take it", err.message));
            ent.reload();
          }}
        >
          <UserMinus />
        </Button>
      ),
    },
  ];
  return (
    <Section title="Entitlements" note="What a member may do here beyond opening it. Workspace admins have them all.">
      <form
        className="flex flex-wrap items-center gap-2"
        onSubmit={async (e) => {
          e.preventDefault();
          try {
            await api.entitle(ws, who, what);
            ent.reload();
          } catch (err) {
            toast.error("Could not give it", (err as Error).message);
          }
        }}
      >
        <Select className="w-full sm:w-64" aria-label="Member" placeholder="Member" value={who} onValueChange={setWho} options={(members.data ?? []).map((m) => ({ value: m.principal, label: m.principal }))} />
        <Select className="w-44" aria-label="Entitlement" placeholder="Entitlement" value={what} onValueChange={setWhat} options={available.map((a) => ({ value: a, label: a }))} />
        <Button variant="primary" type="submit" disabled={!who || !what}>
          Give
        </Button>
      </form>
      <DataTable columns={columns} rows={ent.data?.items ?? []} rowKey={(e) => `${e.principal}/${e.entitlement}`} count={`${(ent.data?.items ?? []).length} given`} empty="No entitlements given." />
    </Section>
  );
}

function Danger({ ws, workspace }: { ws: string; workspace: api.Workspace }) {
  const [name, setName] = useState(workspace.name);
  return (
    <Section title="Workspace" note="Only account admins can rename or delete a workspace. A workspace that owns catalogs cannot be deleted: drop them first.">
      <div className="flex flex-wrap items-center gap-2">
        <Input className="w-64" value={name} onChange={(e) => setName(e.target.value)} aria-label="Workspace name" />
        <Button
          disabled={!name.trim() || name === workspace.name}
          onClick={async () => {
            try {
              await api.renameWorkspace(ws, name);
              toast.success("Renamed");
              location.reload();
            } catch (e) {
              toast.error("Could not rename", (e as Error).message);
            }
          }}
        >
          Rename
        </Button>
      </div>
      <div className="flex items-center justify-between gap-3 rounded-lg border border-border p-3">
        <div>
          <h3 className="text-sm font-semibold">Delete this workspace</h3>
          <p className="text-sm text-muted">Everyone is unassigned and its settings go.</p>
        </div>
        <Button
          variant="danger"
          onClick={async () => {
            if (!(await confirmAction({ title: `Delete ${workspace.name}?`, description: "People lose access to this workspace.", confirmLabel: "Delete", danger: true }))) return;
            try {
              await api.deleteWorkspace(ws);
              // The workspace is gone, so there is nothing here to stay on: go to the account site.
              location.assign(centralOrigin);
            } catch (e) {
              toast.error("Could not delete it", (e as Error).message);
            }
          }}
        >
          <Trash2 /> Delete
        </Button>
      </div>
    </Section>
  );
}
