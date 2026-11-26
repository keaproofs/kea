//! Reviewing a whole batch of finished projects, and measuring the reviewer against a hand check.

use alloy_primitives::Address;
use anyhow::{Context, Result};
use kea_chain::{ROBINHOOD_CHAIN_ID, Rpc};
use kea_claims::messages::Client;
use kea_claims::model::Model;
use kea_claims::prompt;
use kea_claims::review::Reviewer;
use kea_core::{ContractRun, Reviews};
use kea_engine::review::{REVIEW_FILE, review};
use kea_sources::verified::Explorer;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// How the reviewer's calls compare with hand labels, where a labelled verdict means the claim
/// failed the hand check and no label means it passed.
#[derive(Debug, Default)]
struct Agreement {
    caught: usize,
    missed: usize,
    false_alarms: usize,
    cleared: usize,
}

/// Reviews every project in a batch (`<dir>/<address>/result.json`) and writes each project's
/// review file; with `labels`, also reports agreement with a hand check.
///
/// # Errors
/// When the API key is missing, a project cannot be read, or a subject cannot be fetched.
pub async fn review_batch(dir: &Path, model: Model, labels: Option<&Reviews>, rpc_url: &str) -> Result<String> {
    let client = Client::from_env().context("ANTHROPIC_API_KEY is not set")?;
    let reviewer = Reviewer::new(client, model);
    let rpc = Rpc::new(rpc_url.to_string());
    let explorer = Explorer::new(ROBINHOOD_CHAIN_ID);
    let mut projects: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.join("result.json").is_file())
        .collect();
    projects.sort();
    let (mut out, mut agreement, mut spent) = (String::new(), Agreement::default(), 0.0);
    for project in projects {
        let run: ContractRun = serde_json::from_str(&fs::read_to_string(project.join("result.json"))?)
            .with_context(|| format!("parsing {}", project.join("result.json").display()))?;
        let test_source = fs::read_to_string(project.join("test/Proof.t.sol")).unwrap_or_default();
        let address: Address = run.address.parse().with_context(|| format!("address {:?}", run.address))?;
        let subject = kea_sources::fetch(&rpc, &explorer, address).await.context("fetching the subject")?;
        let getters = prompt::live_getters(&rpc, subject.address).await;
        let checked = review(&reviewer, &prompt::subject(&subject, &getters, None), &run, &test_source).await;
        spent += checked.cost_usd;
        fs::write(project.join(REVIEW_FILE), serde_json::to_string_pretty(&checked.entries)?)?;
        let name = run.name.rsplit(':').next().unwrap_or(&run.name);
        for (key, entry) in &checked.entries {
            let claim_id = key.rsplit_once(':').map_or(key.as_str(), |(_, id)| id);
            let failed = entry.get("verdict").is_some();
            let hand = labels.map(|labels| labels.get(&run.address, claim_id).is_some());
            match hand {
                Some(true) if failed => agreement.caught += 1,
                Some(true) => agreement.missed += 1,
                Some(false) if failed => agreement.false_alarms += 1,
                Some(false) => agreement.cleared += 1,
                None => {}
            }
            let hand = hand.map_or("", |bad| if bad { "  hand: mismatch" } else { "  hand: ok" });
            let finding = entry["finding"].as_str().unwrap_or("");
            let note = entry["note"].as_str().unwrap_or("");
            let _ = writeln!(out, "{name:30} {claim_id:18} {finding:10}{hand}\n      {note}");
        }
    }
    if labels.is_some() {
        let Agreement { caught, missed, false_alarms, cleared } = agreement;
        let _ = writeln!(
            out,
            "\nhand mismatches caught {caught}, missed {missed} | hand passes kept {cleared}, flagged {false_alarms}"
        );
    }
    let _ = writeln!(out, "review cost: ${spent:.2}");
    Ok(out)
}
