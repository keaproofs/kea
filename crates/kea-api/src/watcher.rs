//! The watcher: proved code must still be the code at its address. A proxy can be pointed at new
//! code at any time, and a report about the old code would then describe something that no longer
//! runs. Each pass compares the code hash every report was proved on with what runs now.

use crate::routes::{App, identity, within_budget};
use crate::store::{NewJob, now};
use alloy_primitives::Address;
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// Who the watcher's re-checks are booked to; the per-requester limit is for people.
const WATCHER: &str = "watcher";

/// What the stale record says when no code runs at the address any more.
const NO_CODE: &str = "none";

/// The least time between two re-checks of one address, so code that cannot be checked yet
/// (unverified, say) does not spend the budget on every pass.
const RETRY_SECONDS: i64 = 24 * 60 * 60;

/// Runs a pass every configured interval until the process ends.
pub async fn run(app: Arc<App>) {
    let every = Duration::from_secs(app.config.watch_minutes.max(1) * 60);
    loop {
        tokio::time::sleep(every).await;
        if let Err(error) = pass(&app).await {
            eprintln!("watcher: {error:#}");
        }
    }
}

/// One pass over every proved address. Returns how many changes it found that no pass had seen.
///
/// # Errors
/// When the store fails. A chain read that fails skips that address until the next pass.
pub async fn pass(app: &App) -> Result<usize> {
    let mut changed = 0;
    for (address, proved_on) in app.store.watched()? {
        let Ok(parsed) = address.parse::<Address>() else { continue };
        // No code at all is a change too: the contract was destroyed, or its proxy points at nothing.
        let (implementation, hash) = match identity(&app.rpc, parsed).await {
            Ok(Some((implementation, hash))) => (implementation, format!("{hash:#x}")),
            Ok(None) => (None, NO_CODE.to_string()),
            Err(_) => continue,
        };
        if hash == proved_on {
            continue;
        }
        let implementation = implementation.map(|implementation| format!("{implementation:#x}"));
        if app.store.mark_stale(&address, &hash)? {
            changed += 1;
            app.store.add_event(
                &address,
                "code-changed",
                &json!({"from": proved_on, "to": hash, "implementation": implementation}),
            )?;
        }
        // Until a re-check lands, the report stays marked and the watcher keeps asking, once a day.
        let asked_lately = app.store.last_asked(&address)?.is_some_and(|at| now() - at < RETRY_SECONDS);
        if hash != NO_CODE && !asked_lately && app.store.open_job_for(&address)?.is_none() && within_budget(app)? {
            app.store.enqueue(&NewJob {
                address: &address,
                question: None,
                requester: WATCHER,
                code_hash: &hash,
                implementation: implementation.as_deref(),
            })?;
            app.wake.notify_one();
        }
    }
    Ok(changed)
}
