import { Building2, ChevronRight, Layers } from "lucide-react";
import { useState } from "react";
import { Badge, Button, Input, Page, PageEmpty, Section, toast } from "@/design";
import * as api from "@/api";
import { Invitation } from "@/components/Invitation";
import { openWorkspace } from "@/site";
import { go } from "@/route";

/** Where sign-in lands: who you are, the workspaces you can open, the organizations you belong to. */
export function Home({ me, reload }: { me: api.Me | undefined; reload: () => void }) {
  if (!me) return <Page title="Home">{null}</Page>;
  const first = (me.name || me.email || me.subject).split(/[ @]/)[0];
  return (
    <Page title={`Welcome, ${first}`} subtitle={me.email || `Signed in as ${me.subject}`}>
      <Section title="Your workspaces" description="Where you work with data. Open one to browse catalogs, files and who can reach them.">
        {me.workspaces.length === 0 ? (
          <PageEmpty icon={Layers} title="You are not in a workspace yet">
            Ask an account admin of your organization to add you to one.
          </PageEmpty>
        ) : (
          <div className="-m-1 divide-y divide-border">
            {me.workspaces.map((w) => (
              <button key={w.id} type="button" onClick={() => openWorkspace(w.id)} className="flex w-full cursor-pointer items-center gap-3 rounded-lg px-3 py-3 text-left transition-colors hover:bg-card-hover">
                <Layers className="shrink-0 text-muted" aria-hidden />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm font-medium text-foreground">{w.name}</span>
                  <span className="block truncate text-xs text-muted">{w.org_alias === "Personal" ? "Your own space" : w.org_alias}</span>
                </span>
                <Badge tone={w.admin ? "accent" : "neutral"}>{w.admin ? "Admin" : "User"}</Badge>
                <ChevronRight className="shrink-0 text-muted" aria-hidden />
              </button>
            ))}
          </div>
        )}
      </Section>

      <Section title="Organizations" description="Your own space, and the organizations you belong to.">
        <div className="-m-1 divide-y divide-border">
          {me.orgs.map((o) => (
            <div key={o.id} className="flex items-center gap-3 px-3 py-3">
              <Building2 className="shrink-0 text-muted" aria-hidden />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium text-foreground">{o.alias}</span>
                <span className="block text-xs text-muted">{o.kind === "personal" ? "Your own space" : o.has_account ? "Organization" : "Not set up yet: an admin of it signs in first"}</span>
              </span>
              {o.account_admin && (
                <Button variant="secondary" onClick={() => go({ screen: "account", org: o.id, tab: "workspaces" })}>
                  Manage
                </Button>
              )}
            </div>
          ))}
        </div>
      </Section>

      {me.platform_admin && <NewOrganization reload={reload} />}
    </Page>
  );
}

/** For the installation's admin: make an organization and its first admin. */
function NewOrganization({ reload }: { reload: () => void }) {
  const [alias, setAlias] = useState("");
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [invited, setInvited] = useState<api.Invited | null>(null);
  return (
    <Section title="New organization" description="You manage this installation. Make an organization and its first admin, who then adds the rest of their people.">
      <form
        className="flex flex-wrap items-center gap-2"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          try {
            const made = await api.createOrganization(alias.trim(), email.trim(), name.trim());
            toast.success(`Created ${made.alias}`);
            setInvited(made.admin);
            setAlias("");
            setEmail("");
            setName("");
            reload();
          } catch (err) {
            toast.error("Could not create it", (err as Error).message);
          } finally {
            setBusy(false);
          }
        }}
      >
        <Input className="min-w-40 max-w-48" placeholder="Name, such as acme" aria-label="Organization name" value={alias} onChange={(e) => setAlias(e.target.value)} />
        <Input className="min-w-56 max-w-xs" type="email" placeholder="First admin's email" aria-label="First admin's email" value={email} onChange={(e) => setEmail(e.target.value)} />
        <Input className="min-w-40 max-w-48" placeholder="Their name" aria-label="First admin's name" value={name} onChange={(e) => setName(e.target.value)} />
        <Button variant="primary" type="submit" disabled={busy || !alias.trim() || !email.trim() || !name.trim()}>
          Create
        </Button>
      </form>
      <Invitation invited={invited} onClose={() => setInvited(null)} />
    </Section>
  );
}
