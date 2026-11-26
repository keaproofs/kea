// The Kea API, as the site reads it.

export type Status = "proven" | "found" | "unclear";
export type Family = "vault" | "token" | "market" | "payout" | "upgradeable" | "admin";

export interface Holder {
  address: string;
  /** "EOA", "Safe 2", "contract", "none" or "unknown". */
  kind: string;
}

export interface Claim {
  id: string;
  kind: string | null;
  family: Family | null;
  plain: string;
  scope: string;
  status: Status;
  verdict: string;
  why: string;
  note: string | null;
  holder: Holder | null;
  check: string;
  replay: string | null;
}

export interface Summary {
  proven: number;
  found: number;
  unclear: number;
}

export interface Report {
  address: string;
  proved_address: string;
  twin_of: string | null;
  name: string;
  implementation: string | null;
  code_hash: string;
  checked_at: number;
  summary: Summary;
  claims: Claim[];
  drafts: number;
  prove_seconds: number;
  /** The test file's path; null while a claim in it is held for private disclosure. */
  test: string | null;
  stale_since: number | null;
}

export interface Listed {
  address: string;
  name: string;
  checked_at: number;
  summary: Summary;
  /** One power found, then proofs, shortest first. */
  top: { id: string; plain: string; status: Status }[];
  /** The code changed since the report: its counts describe code that no longer runs. */
  stale?: boolean;
}

export interface Job {
  job: number;
  address: string;
  state: "queued" | "running" | "done" | "failed";
  position: number | null;
  error: string | null;
  report: string | null;
}

/** What asking returns: a report ready to read, or a job to follow. */
export type Asked = { kind: "ready"; address: string } | { kind: "job"; job: Job };

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
  }
}

async function read<T>(response: Response): Promise<T> {
  const body = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new ApiError(body.error ?? "Kea did not answer. Try again in a moment.", response.status);
  }
  return body as T;
}

export async function recent(limit = 20): Promise<Listed[]> {
  const body = await read<{ reports: Listed[] }>(await fetch(`/v1/contracts?limit=${limit}`));
  return body.reports;
}

/** Reports already asked for, kept a minute: a page opened from a link the pointer rested on has
 * its data waiting. A failed answer is dropped, so the next ask tries again. */
const asked = new Map<string, { at: number; answer: Promise<Report> }>();

export function report(address: string): Promise<Report> {
  const key = address.toLowerCase();
  const held = asked.get(key);
  if (held && Date.now() - held.at < 60_000) return held.answer;
  const answer = fetch(`/v1/contracts/${key}`).then((response) => read<Report>(response));
  answer.catch(() => asked.delete(key));
  asked.set(key, { at: Date.now(), answer });
  return answer;
}

export async function ask(address: string, question?: string, fresh = false): Promise<Asked> {
  const response = await fetch("/v1/asks", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ address, question: question || undefined, fresh }),
  });
  const body = await read<Partial<Job> & { state: string; report?: string }>(response);
  if (response.status === 200 && body.state === "done") {
    return { kind: "ready", address: address.trim().toLowerCase() };
  }
  return { kind: "job", job: body as Job };
}

export async function job(id: number): Promise<Job> {
  return read<Job>(await fetch(`/v1/asks/${id}`));
}

export const isAddress = (text: string) => /^0x[0-9a-fA-F]{40}$/.test(text.trim());

export const short = (address: string) => `${address.slice(0, 6)}…${address.slice(-4)}`;

/** Public times are UTC: readers are everywhere. */
export function checkedOn(seconds: number): string {
  const date = new Date(seconds * 1000);
  const day = date.toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" });
  const time = date.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", timeZone: "UTC" });
  return `${day}, ${time} UTC`;
}
