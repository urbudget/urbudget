import { ApiError, sha256 } from "../util";

export const MAX_BYTES = 50 * 1024 * 1024;
export const MAX_TOTAL_BYTES = 1024 * 1024 * 1024;
export const MAX_STAGING_BYTES = 200 * 1024 * 1024;
export const MAX_BUDGETS = 50;
export const MAX_INSTALLATIONS = 100;
export const STAGING_MS = 60 * 60_000;
export const RETENTION_MS = 30 * 86_400_000;
export const MAX_COUNTER = 9223372036854775806n;
export type Kind = "create" | "update" | "trash" | "restore" | "purge" | "keySetup" | "keyReset" | "migrateProtocol";
export type Payload = { format: string; sha256: string | null; length: number; meta: string; context: Record<string, unknown> };
export type Manifest = {
  accountId: string; installationId: string; operationId: string; kind: Kind;
  keyGeneration: string | null; catalogEpoch: string | null;
  budgetId?: string; incarnation?: string; expectedVersion?: string; newIncarnation?: string;
  payload?: Payload; source?: { incarnation: string; version: string; sha256: string | null };
  expectedProtocol?: number; key?: { salt: string; check: string };
};
export type Head = {
  budgetId: string; incarnation: string; version: string; keyGeneration: string;
  state: "active" | "trashed" | "purged"; payload: Payload | null;
  updatedSequence: string; updatedAt: string; deletedAt: string | null; retainedUntil: string | null; device: string;
};
export type Receipt = {
  sequence: string; digest: string; operationId: string; kind: Kind;
  outcome: "accepted" | "rejected" | "cancelled" | "expired"; code: string;
  accountId: string; keyGeneration: string | null; catalogEpoch: string; protocol: number;
  budget: Head | null;
};
export const hasBlob = (kind: Kind) => kind === "create" || kind === "update" || kind === "restore";
export const isControl = (kind: Kind) => kind === "keySetup" || kind === "keyReset" || kind === "migrateProtocol";
export const validId = (value: unknown): value is string => typeof value === "string" && /^[A-Za-z0-9_-]{1,128}$/.test(value);
export function counter(value: unknown, name = "counter"): string {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value) || BigInt(value) > MAX_COUNTER)
    throw new ApiError(400, "invalid_counter", `${name} must be a non-negative decimal integer string.`);
  return value;
}
export function canonical(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") return `{${Object.keys(value).filter(k => (value as Record<string, unknown>)[k] !== undefined).sort().map(k => `${JSON.stringify(k)}:${canonical((value as Record<string, unknown>)[k])}`).join(",")}}`;
  return JSON.stringify(value);
}
export const manifestDigest = (manifest: unknown) => sha256(canonical(manifest));
const invalid = (message: string): never => { throw new ApiError(400, "invalid_manifest", message); };
function fields(value: unknown, allowed: string[]) {
  return !!value && typeof value === "object" && !Array.isArray(value) && Object.keys(value).every(k => allowed.includes(k));
}
/** Cancellation never interprets or publishes payload/control fields. It only
 * authenticates whose exact immutable manifest is being terminally abandoned. */
export function parseCancellationManifest(value: Record<string, unknown>, account: string, installation: string) {
  if (value.accountId !== account || value.installationId !== installation || !validId(value.operationId) ||
      !["create", "update", "trash", "restore", "purge", "keySetup", "keyReset", "migrateProtocol"].includes(value.kind as string))
    invalid("The cancellation identity is invalid.");
  return value;
}
export function parseManifest(value: Record<string, unknown>, account: string, installation: string): Manifest {
  const allowed = new Set(["accountId", "installationId", "operationId", "kind", "keyGeneration", "catalogEpoch", "budgetId", "incarnation", "expectedVersion", "newIncarnation", "payload", "source", "expectedProtocol", "key"]);
  if (Object.keys(value).some(k => !allowed.has(k))) invalid("The manifest contains an unsupported field.");
  if (value.accountId !== account || value.installationId !== installation) invalid("The manifest belongs to another account or installation.");
  if (!validId(value.operationId)) invalid("The operation ID is invalid.");
  const kinds: Kind[] = ["create", "update", "trash", "restore", "purge", "keySetup", "keyReset", "migrateProtocol"];
  if (!kinds.includes(value.kind as Kind)) invalid("The mutation kind is invalid.");
  if (value.keyGeneration !== null) counter(value.keyGeneration, "keyGeneration");
  if (value.catalogEpoch !== null && !validId(value.catalogEpoch)) invalid("The catalog epoch is invalid.");
  const m = value as Manifest;
  if (isControl(m.kind)) {
    if ([m.budgetId, m.payload, m.source, m.incarnation, m.newIncarnation, m.expectedVersion].some(v => v !== undefined)) invalid("Account controls cannot contain budget data.");
    if (m.expectedProtocol !== 1 && m.expectedProtocol !== 2) invalid("Account controls require the expected protocol.");
    if (m.kind === "migrateProtocol" && m.key !== undefined) invalid("Migration preserves the current key details.");
    if (m.kind !== "migrateProtocol" && (!fields(m.key, ["salt", "check"]) || !m.key || typeof m.key.salt !== "string" || !/^[A-Za-z0-9_-]{16,128}$/.test(m.key.salt) || typeof m.key.check !== "string" || !m.key.check || m.key.check.length > 1024)) invalid("The proposed key details are invalid.");
  } else {
    if (!validId(m.budgetId)) invalid("The budget ID is invalid.");
    if (m.key !== undefined || m.expectedProtocol !== undefined || (m.kind !== "restore" && m.source !== undefined)) invalid("The mutation contains unrelated control data.");
    if (m.keyGeneration === null || m.catalogEpoch === null) invalid("Budget mutations require an unlocked generation and catalog epoch.");
    counter(m.expectedVersion, "expectedVersion");
    if (m.kind !== "create" && m.kind !== "restore" && m.newIncarnation !== undefined) invalid("Only create or restore may establish a new incarnation.");
    if (m.kind === "create") {
      if (m.incarnation !== undefined || m.expectedVersion !== "0" || !validId(m.newIncarnation)) invalid("Create requires version zero and a new incarnation.");
    } else if (!validId(m.incarnation)) invalid("The expected incarnation is required.");
    if (m.kind === "restore" && (!validId(m.newIncarnation) || m.newIncarnation === m.incarnation || !fields(m.source, ["incarnation", "version", "sha256"]) || !m.source || m.source.incarnation !== m.incarnation || m.source.version !== m.expectedVersion || (m.source.sha256 !== null && (typeof m.source.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(m.source.sha256))))) invalid("Restore requires the retained source and a fresh incarnation.");
    if (hasBlob(m.kind)) {
      const p = m.payload;
      if (!fields(p, ["format", "sha256", "length", "meta", "context"]) || !p || p.format !== "v2" || typeof p.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(p.sha256) || !Number.isSafeInteger(p.length) || p.length < 1 || p.length > MAX_BYTES || typeof p.meta !== "string" || p.meta.length > 2048 || !/^[A-Za-z0-9_-]+$/.test(p.meta) || p.meta.length % 4 === 1 || !fields(p.context, ["format", "accountId", "keyGeneration", "budgetId", "incarnation", "operationId", "parentVersion"]) || Object.values(p.context).some(v => typeof v !== "string")) invalid("The encrypted payload descriptor is invalid.");
      const expected = { format: "v2", accountId: account, keyGeneration: m.keyGeneration, budgetId: m.budgetId, incarnation: m.newIncarnation ?? m.incarnation, operationId: m.operationId, parentVersion: m.expectedVersion };
      if (canonical(p!.context) !== canonical(expected)) invalid("The payload's authenticated context does not match the mutation.");
    } else if (m.payload !== undefined) invalid("This mutation cannot replace content.");
  }
  return m;
}
