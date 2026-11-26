import { useEffect, useId, useRef, useState } from "react";
import { ApiError, ask, isAddress, job as readJob, type Job } from "../api";
import { navigate } from "../router";

type State =
  | { kind: "idle" }
  | { kind: "sending" }
  | { kind: "waiting"; job: Job }
  | { kind: "error"; message: string };

const STAGE: Record<Job["state"], string> = {
  queued: "In the queue",
  running: "Taking it apart: drafting claims, proving, replaying, reviewing the words",
  done: "Done. Opening the report",
  failed: "Could not finish",
};

/** The feed slot: paste an address, get a report, or follow the job that makes one. */
export function AskSlot({
  initial = "",
  onJob,
}: {
  initial?: string;
  onJob?: (job: Job | null) => void;
}) {
  const [address, setAddress] = useState(initial);
  const [question, setQuestion] = useState("");
  const [asking, setAsking] = useState(false);
  const [state, setState] = useState<State>({ kind: "idle" });
  const timer = useRef<number | undefined>(undefined);
  const ids = { address: useId(), question: useId(), status: useId() };

  useEffect(() => () => window.clearTimeout(timer.current), []);

  function follow(current: Job) {
    setState({ kind: "waiting", job: current });
    onJob?.(current);
    if (current.state === "done") {
      // One beat on the stamp before the report opens, so the print visibly finishes.
      timer.current = window.setTimeout(() => {
        onJob?.(null);
        navigate(`/c/${current.address}`);
      }, 900);
      return;
    }
    if (current.state === "failed") {
      onJob?.(null);
      setState({ kind: "error", message: current.error ?? "The check could not finish." });
      return;
    }
    timer.current = window.setTimeout(async () => {
      try {
        follow(await readJob(current.job));
      } catch {
        timer.current = window.setTimeout(() => follow(current), 4000);
      }
    }, 3000);
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    const trimmed = address.trim();
    if (!isAddress(trimmed)) {
      setState({ kind: "error", message: "That is not a contract address: it should be 0x and 40 hex characters." });
      return;
    }
    setState({ kind: "sending" });
    try {
      const answer = await ask(trimmed, asking ? question.trim() : undefined);
      if (answer.kind === "ready") navigate(`/c/${answer.address}`);
      else follow(answer.job);
    } catch (error) {
      setState({ kind: "error", message: error instanceof ApiError ? error.message : "Kea did not answer. Try again." });
    }
  }

  const busy = state.kind === "sending" || state.kind === "waiting";
  return (
    <form className="slot" onSubmit={submit} aria-describedby={ids.status}>
      <label className="visually-hidden" htmlFor={ids.address}>
        Contract address
      </label>
      <div className="slot-row">
        <input
          id={ids.address}
          className="slot-input"
          value={address}
          onChange={(event) => setAddress(event.target.value)}
          placeholder="0x… contract address"
          spellCheck={false}
          autoComplete="off"
          inputMode="text"
          disabled={busy}
        />
        <button className="slot-button" type="submit" disabled={busy}>
          {busy ? "Working…" : "Take it apart"}
        </button>
      </div>
      {asking ? (
        <div className="slot-question">
          <label htmlFor={ids.question}>Your question, answered first</label>
          <input
            id={ids.question}
            value={question}
            maxLength={280}
            onChange={(event) => setQuestion(event.target.value)}
            placeholder="Can the owner stop me from selling?"
            disabled={busy}
          />
        </div>
      ) : (
        <button type="button" className="slot-more" onClick={() => setAsking(true)}>
          Ask something specific
        </button>
      )}
      <p id={ids.status} className={`slot-status slot-status-${state.kind}`} role="status" aria-live="polite">
        {state.kind === "idle" && "Contracts already proven open at once. New code joins a queue and takes a few minutes."}
        {state.kind === "sending" && "Reading what code runs at that address…"}
        {state.kind === "waiting" &&
          (state.job.state === "queued" && state.job.position
            ? `${STAGE.queued}: ${state.job.position} ahead of you.`
            : `${STAGE[state.job.state]}…`)}
        {state.kind === "error" && state.message}
      </p>
    </form>
  );
}
