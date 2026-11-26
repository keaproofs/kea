//! The worker: one job at a time, since proving is CPU-bound and the machine is small.

use crate::routes::App;
use crate::store::{Job, Report, day_start, now};
use alloy_primitives::Address;
use kea_engine::ask;
use std::sync::Arc;
use std::time::Duration;

/// Runs queued jobs until the process ends.
pub async fn run(app: Arc<App>) {
    loop {
        let next = fits_today(&app).and_then(|fits| if fits { app.store.claim_next() } else { Ok(None) });
        match next {
            Ok(Some(job)) => prove(&app, job).await,
            // A queued job wakes the worker; the timeout is only a safety net.
            Ok(None) => {
                let _ = tokio::time::timeout(Duration::from_secs(30), app.wake.notified()).await;
            }
            Err(error) => {
                eprintln!("worker: {error:#}");
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        }
    }
}

/// Whether a job's whole budget still fits today's. Admission counts open jobs at a typical cost;
/// this makes the day's budget a hard cap, give or take the one draft a job may finish past its own.
/// A job that does not fit waits in the queue for the next UTC day.
fn fits_today(app: &App) -> anyhow::Result<bool> {
    let budget = app.config.daily_budget_usd;
    Ok(app.store.spent_since(day_start(now()))? + app.config.job_budget_usd.min(budget) <= budget)
}

async fn prove(app: &App, job: Job) {
    let Ok(address) = job.address.parse::<Address>() else {
        let _ = app.store.finish(job.id, Err("the stored address does not parse"), 0.0);
        return;
    };
    let meter = ask::Meter::default();
    let work = app.config.work.join(job.id.to_string());
    let request = ask::Request {
        address,
        question: job.question.clone(),
        model: app.config.model,
        review_model: app.config.model,
        max_attempts: app.config.attempts,
        budget_usd: app.config.job_budget_usd,
        work: work.clone(),
        rpc_url: app.config.rpc_url.clone(),
        meter: meter.clone(),
    };
    let outcome = match ask::ask(request).await {
        // Every answer declined or unusable: nothing ran, so nothing is proved, and a report of
        // nothing would only hide that.
        Ok(answer) if answer.test_source.is_empty() => Err("no draft could run".to_string()),
        Ok(answer) => {
            let cost = answer.run.cost_usd;
            let report = Report {
                address: job.address.clone(),
                name: answer.run.name.rsplit(':').next().unwrap_or(&answer.run.name).to_string(),
                implementation: job.implementation.clone(),
                code_hash: job.code_hash.clone(),
                run: answer.run,
                test_source: answer.test_source,
                review: answer.review.entries,
                question: job.question.clone(),
                cost_usd: cost,
                checked_at: now(),
            };
            let private: Vec<String> =
                crate::view::withheld_claims(&report).into_iter().map(|judged| judged.claim.id).collect();
            let saved = app.store.save_report(&report);
            if saved.is_ok() && !private.is_empty() {
                eprintln!("job {}: {} claims held for private disclosure", job.id, private.len());
                let _ =
                    app.store.add_event(&report.address, "disclosure-held", &serde_json::json!({"claims": private}));
            }
            saved.map_err(|error| format!("saving the report: {error:#}"))
        }
        Err(error) => Err(format!("{error:#}")),
    };
    if let Err(error) = &outcome {
        eprintln!("job {}: {error}", job.id);
    }
    if let Err(error) = app.store.finish(job.id, outcome.as_ref().copied().map_err(String::as_str), meter.usd()) {
        eprintln!("job {}: recording the outcome: {error:#}", job.id);
    }
    // The report holds what matters; a finished project is only disk. A failed one stays for a look.
    if outcome.is_ok() {
        let _ = std::fs::remove_dir_all(&work);
    }
}
