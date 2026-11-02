import { useState } from "react";
import { Input, Select } from "@/design";
import { toPrincipal, typeLabel, type PrincipalType } from "@/principals";

/** Names a person, group or service principal to give something to. Reports the typed id (`user:…`), or "" until it is filled in. */
export function PrincipalField({ onChange, className = "", size = "md" }: { onChange: (principal: string) => void; className?: string; size?: "sm" | "md" }) {
  const [type, setType] = useState<PrincipalType>("user");
  const [id, setId] = useState("");
  const hint = type === "group" ? "/finance/analysts" : type === "sp" ? "Keycloak service account id" : "Keycloak user id";
  return (
    <div className={`flex min-w-0 ${size === "sm" ? "" : "flex-1"} items-center gap-2 ${className}`}>
      <Select
        size={size}
        className={size === "sm" ? "w-32 shrink-0" : "w-44 shrink-0"}
        aria-label="Kind of principal"
        value={type}
        onValueChange={(v) => {
          setType(v as PrincipalType);
          onChange(id ? toPrincipal(v as PrincipalType, id) : "");
        }}
        options={(Object.keys(typeLabel) as PrincipalType[]).map((t) => ({ value: t, label: typeLabel[t] }))}
      />
      <Input
        size={size}
        className="min-w-0 flex-1"
        aria-label="Id"
        placeholder={hint}
        value={id}
        spellCheck={false}
        onChange={(e) => {
          setId(e.target.value);
          onChange(e.target.value.trim() ? toPrincipal(type, e.target.value) : "");
        }}
      />
    </div>
  );
}
