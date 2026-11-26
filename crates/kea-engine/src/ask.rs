//! Asking a contract: draft claims and checks, prove them, and redraft from the evidence until
//! enough claims are decided or the attempts or the budget run out.

use crate::prove::{self, Workspace};
use crate::review::{self, AttemptReview, REVIEW_FILE};
use alloy_primitives::Address;
use anyhow::{Context, Result};
use kea_claims::messages::Client;
use kea_claims::model::Model;
use kea_claims::review::Reviewer;
use kea_claims::{Drafted, Drafter, Unusable, prompt};
use kea_core::run::{Attempt, MIN_PROVEN};
use kea_core::{ContractRun, Scored, Verdict};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// What to ask and how much to spend.
pub struct Request {
    /// The deployed address.
    pub address: Address,
    /// A person's question, answered first.
    pub question: Option<String>,
    /// The drafting model.
    pub model: Model,
    /// The model that reviews each attempt's proven claims.
    pub review_model: Model,
    /// The most drafts to try.
    pub max_attempts: u32,
    /// No new draft starts once this much has been spent, in US dollars.
    pub budget_usd: f64,
    /// The directory that receives the project.
    pub work: PathBuf,
    /// The chain endpoint.
    pub rpc_url: String,
    /// Kept up to date with what the job has spent, so a job that fails still accounts for it.
    pub meter: Meter,
}

/// What a job has spent so far, readable while it runs and after it fails.
#[derive(Clone, Debug, Default)]
pub struct Meter(std::sync::Arc<std::sync::atomic::AtomicU64>);

impl Meter {
    fn set(&self, usd: f64) {
        self.0.store(usd.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    /// US dollars billed so far.
    pub fn usd(&self) -> f64 {
        f64::from_bits(self.0.load(std::sync::atomic::Ordering::Relaxed))
    }
}

/// The kept attempt, carrying every attempt's record and the whole drafting cost.
pub struct Answer {
    /// The run with the most proven claims; else the last one that ran; else a record with
    /// nothing run, so every answer that was billed is accounted for.
    pub run: ContractRun,
    /// The kept test file; empty when no draft ran.
    pub test_source: String,
    /// The review of the kept attempt's proven claims.
    pub review: AttemptReview,
}

/// An attempt worth keeping.
struct Kept {
    /// Whether it compiled, then meaningful claims proven after review.
    rank: (bool, usize),
    run: ContractRun,
    test_source: String,
    review: AttemptReview,
}

/// Drafts, proves and redrafts.
///
/// # Errors
/// When the subject cannot be prepared, the API key is missing, the first request gets no
/// answer, or a tool cannot be started.
pub async fn ask(request: Request) -> Result<Answer> {
    let client = Client::from_env().context("ANTHROPIC_API_KEY is not set")?;
    let drafter = Drafter::new(client.clone(), request.model);
    let reviewer = Reviewer::new(client, request.review_model);
    let workspace = prove::prepare(request.address, &request.work, &request.rpc_url).await?;
    let getters = prompt::live_getters(&workspace.rpc, workspace.subject.address).await;
    let subject_text = prompt::subject(&workspace.subject, &getters, None);

    let mut log: Vec<Attempt> = Vec::new();
    let mut kept: Option<Kept> = None;
    let mut last_file: Option<String> = None;
    let mut feedback: Option<String> = None;
    let mut spent = 0.0;
    for number in 1..=request.max_attempts {
        if spent >= request.budget_usd {
            break;
        }
        let written = match drafter.draft(&subject_text, request.question.as_deref(), feedback.as_deref()).await {
            Ok(written) => written,
            // Nothing was billed for this one; what the earlier attempts found is kept.
            Err(error) if !log.is_empty() => {
                eprintln!("attempt {number}: no answer from the API, stopping: {error}");
                break;
            }
            Err(error) => return Err(error).context("drafting"),
        };
        spent += written.cost_usd;
        request.meter.set(spent);
        let mut record = draft_record(number, &written);
        let draft = match written.answer {
            Ok(draft) => draft,
            Err(unusable) => {
                record.insert("unusable".into(), json!(unusable.to_string()));
                log.push(attempt_record(record, None));
                // The server already offered the request to its fallback model.
                if matches!(unusable, Unusable::Declined(_)) {
                    break;
                }
                feedback = Some(prompt::unusable(&unusable.to_string(), last_file.as_deref()));
                continue;
            }
        };
        last_file = Some(draft.test_sol.clone());
        let problems = draft.problems();
        if !problems.is_empty() {
            snapshot(&workspace, number, &draft.test_sol, None, None)?;
            record.insert("refused".into(), json!(problems));
            log.push(attempt_record(record, None));
            feedback = Some(prompt::rejected(&problems.join("\n"), &draft.test_sol));
            continue;
        }
        let run = workspace.attempt(draft.claims.clone(), &draft.test_sol).await?;
        if !built(&run) {
            snapshot(&workspace, number, &draft.test_sol, Some(&run), None)?;
            log.push(attempt_record(record, run.status.clone()));
            let compiler = format!("{}\n{}", workspace.log("build.log"), workspace.log("build.err.log"));
            feedback =
                Some(prompt::rejected(&format!("Compiler output:\n{}", prompt::tail(&compiler, 80)), &draft.test_sol));
            let review = AttemptReview::default();
            keep(&mut kept, Kept { rank: (false, 0), run, test_source: draft.test_sol, review });
            continue;
        }
        let review = review::review(&reviewer, &subject_text, &run, &draft.test_sol).await;
        spent += review.cost_usd;
        request.meter.set(spent);
        snapshot(&workspace, number, &draft.test_sol, Some(&run), Some(&review))?;
        let scored = run.score(&draft.test_sol, &review.reviews());
        let proven = scored.proven();
        record.insert("proven".into(), json!(proven));
        record.insert("review_cost".into(), json!(review.cost_usd));
        record.insert("taken_by_review".into(), json!(review.notes.len()));
        log.push(attempt_record(record, run.status.clone()));
        let notes = review.notes.clone();
        keep(&mut kept, Kept { rank: (true, proven), run, test_source: draft.test_sol.clone(), review });
        if proven >= MIN_PROVEN {
            break;
        }
        feedback = Some(ran_feedback(&workspace, &scored, &notes, &draft.test_sol));
    }

    let (mut run, test_source, review) = match kept {
        Some(Kept { run, test_source, review, .. }) => {
            workspace.write_test(&test_source)?;
            (run, test_source, review)
        }
        None => (workspace.blank(Vec::new()), String::new(), AttemptReview::default()),
    };
    run.attempts = log;
    run.cost_usd = spent;
    workspace.save(&run)?;
    std::fs::write(workspace.dir.join(REVIEW_FILE), serde_json::to_string_pretty(&review.entries)?)?;
    Ok(Answer { run, test_source, review })
}

/// What one drafting answer cost and who wrote it.
fn draft_record(number: u32, written: &Drafted) -> BTreeMap<String, Value> {
    let usage = &written.usage;
    BTreeMap::from([
        ("attempt".to_string(), json!(number)),
        ("model".to_string(), json!(written.served_by)),
        ("cost".to_string(), json!(written.cost_usd)),
        (
            "tokens".to_string(),
            json!({"in": usage.input, "cache_write": usage.cache_write, "cache_read": usage.cache_read, "out": usage.output}),
        ),
    ])
}

/// Feedback after an attempt that compiled and ran: every claim without a usable verdict, the
/// reviewer's reasons, and the prover and replay logs.
fn ran_feedback(workspace: &Workspace, scored: &Scored, review_notes: &[String], test_source: &str) -> String {
    let unusable: Vec<String> = scored
        .claims
        .iter()
        .filter(|judged| !matches!(judged.verdict, Verdict::Proven | Verdict::Finding | Verdict::PowerRenounced))
        .map(|judged| format!("{} ({})", judged.claim.id, judged.verdict))
        .collect();
    prompt::ran(
        scored.proven(),
        &unusable,
        review_notes,
        &workspace.log("halmos.log"),
        &workspace.log("replay.log"),
        test_source,
    )
}

fn built(run: &ContractRun) -> bool {
    run.status.as_ref().and_then(|status| status.get("build")).and_then(Value::as_str) == Some("0")
}

fn attempt_record(extra: BTreeMap<String, Value>, status: Option<BTreeMap<String, Value>>) -> Attempt {
    Attempt { status, extra }
}

/// Keeps `candidate` when it outranks the attempt kept so far; the first of equals stays.
fn keep(kept: &mut Option<Kept>, candidate: Kept) {
    if kept.as_ref().is_none_or(|best| candidate.rank > best.rank) {
        *kept = Some(candidate);
    }
}

/// Keeps each attempt's test file, and its result, tool logs and review when it ran, so every
/// attempt can be traced after the next one overwrites the project.
fn snapshot(
    workspace: &Workspace,
    number: u32,
    test_source: &str,
    run: Option<&ContractRun>,
    review: Option<&AttemptReview>,
) -> Result<()> {
    let dir = workspace.dir.join("attempts").join(number.to_string());
    std::fs::create_dir_all(&dir)?;
    // .txt: forge must not compile old attempts.
    std::fs::write(dir.join("Proof.t.sol.txt"), test_source)?;
    if let Some(run) = run {
        std::fs::write(dir.join("result.json"), serde_json::to_string_pretty(run)?)?;
        // Only the steps this attempt ran left logs: `attempt` clears the earlier ones.
        for log in prove::STEP_LOGS.iter().filter(|log| workspace.dir.join(log).is_file()) {
            std::fs::copy(workspace.dir.join(log), dir.join(log))?;
        }
    }
    if let Some(review) = review {
        std::fs::write(dir.join(REVIEW_FILE), serde_json::to_string_pretty(&review.entries)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(rank: (bool, usize), name: &str) -> Kept {
        let run = serde_json::from_value(json!({"address": "0x0", "name": name, "claims": []})).unwrap();
        Kept { rank, run, test_source: String::new(), review: AttemptReview::default() }
    }

    fn name(kept: Option<&Kept>) -> &str {
        &kept.unwrap().run.name
    }

    #[test]
    fn a_compiled_attempt_outranks_more_proofs_never_and_ties_keep_the_first() {
        let mut kept = None;
        keep(&mut kept, attempt((false, 0), "broken"));
        keep(&mut kept, attempt((true, 0), "compiled"));
        keep(&mut kept, attempt((false, 0), "broken again"));
        assert_eq!(name(kept.as_ref()), "compiled");
        keep(&mut kept, attempt((true, 2), "two"));
        keep(&mut kept, attempt((true, 2), "two later"));
        assert_eq!(name(kept.as_ref()), "two");
        keep(&mut kept, attempt((true, 3), "three"));
        assert_eq!(name(kept.as_ref()), "three");
    }
}
