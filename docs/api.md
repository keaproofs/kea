# API

The service behind https://keaproofs.io. Everything a report shows comes from here, and reading it is free.

- Base URL: `https://keaproofs.io`. Requests and answers are JSON unless a section says otherwise.
- Only `GET` and `POST`. A request body may be at most 4 KB.
- Every error is the HTTP status plus `{"error": "<one plain sentence>"}`.

## Reading reports

### `GET /v1/contracts?limit=<n>`

The latest reports, newest first. `limit` defaults to 20 and stops at 50.

```
curl https://keaproofs.io/v1/contracts?limit=5
```

`{"reports": [...]}`, where each entry has `address`, `name`, `checked_at` (Unix seconds), `summary` (how many claims are
proven, found and unclear) and `top` (up to three claims, each with `id`, `plain` and `status`).

### `GET /v1/contracts/<address>`

One contract's report: `404` if it has not been checked yet.

```
curl https://keaproofs.io/v1/contracts/0x75999324313103e54540dc040050fc823e0ef4ef
```

The report names the contract (`address`, `name`, `implementation` behind a proxy, `code_hash`, `checked_at`) and lists
its `claims`. Each claim has:

| Field | Meaning |
| --- | --- |
| `id` | the claim's catalog id, such as `T4_fee` |
| `plain` | the claim in plain words |
| `kind` | the catalog family it belongs to |
| `scope` | the function it covers, or `any` |
| `verdict` | the exact outcome, such as `proven`, `finding`, `contract-held`, `power-renounced`, `narrow` or `unknown` |
| `why` | that verdict in one sentence |
| `note` | the review's reason, when the review narrowed or refused a proof |
| `holder` | for a power: who holds it (`address`, and `kind`: an account, a Safe, a contract, or nobody) |
| `check` | the name of the test function that decided it |

`twin_of` and `proved_address` say when the report was reused from identical code at another address.

### `GET /v1/contracts/<address>/test`

The Foundry test file that decided the report, as plain text. The README shows how to run it yourself.

### `GET /badge/<address>.svg`

A badge for a README or a site:

```
[![proven by Kea](https://keaproofs.io/badge/<address>.svg)](https://keaproofs.io/c/<address>)
```

### `GET /feed.xml` and `GET /feed/<address>.xml`

Atom feeds: every new report and every change of code under a report, or the same for one contract.

## Asking for a check

### `POST /v1/asks`

```
curl -X POST https://keaproofs.io/v1/asks \
  -H 'Content-Type: application/json' \
  -d '{"address": "0x…", "question": "Can the owner freeze withdrawals?"}'
```

| Field | Meaning |
| --- | --- |
| `address` | the contract, required |
| `question` | up to 280 characters; it steers which claims are drafted and is never published |
| `fresh` | `true` proves this address itself even when identical code was proved elsewhere |

Answers:

| Status | Body | When |
| --- | --- | --- |
| `200` | `{"state": "done", "report": "/v1/contracts/0x…"}` | a current report already exists |
| `200` | `{"state": "done", "twin_of": "0x…", "report": "…"}` | identical code was proved at another address |
| `202` | a job, below | the check is queued or running |
| `400` | | not an address, or a question over 280 characters |
| `422` | | no contract code at that address |
| `429` | | the requester has used today's checks; reports stay free to read |
| `502` | | the chain did not answer |
| `503` | | the queue is full, or today's proving budget is spent |

### `GET /v1/asks/<id>`

A job: `job`, `address`, `state` (`queued`, `running`, `done` or `failed`), `position` while queued, `error` when it
failed, `report` once done, and `created_at`, `started_at`, `finished_at` in Unix seconds.

## Limits

Reading is unlimited. Checks spend model credit, so they are rationed:

- each requester gets a few checks a day; requesters are counted by a salted hash of their address, IPv6 by its /64;
- the queue holds a fixed number of open jobs, and a daily budget caps the model spend;
- identical code is proved once and reused, so a popular contract costs nothing after its first check;
- the counts reset at 00:00 UTC.

## Running your own

`kea-api serve` reads its settings from the environment. Only `KEA_SALT` and `ANTHROPIC_API_KEY` must be set.

| Variable | Default | Meaning |
| --- | --- | --- |
| `KEA_SALT` | none | 32 or more random characters for hashing requesters |
| `ANTHROPIC_API_KEY` | none | the model key that drafting and review use |
| `KEA_LISTEN` | `127.0.0.1:8740` | where the API listens |
| `KEA_DB` | `/var/lib/kea/kea.db` | the SQLite file |
| `KEA_WORK` | `/var/lib/kea/work` | where checks build their Foundry projects |
| `RH_RPC` | the public Robinhood Chain RPC | the chain to read and fork |
| `KEA_MODEL` | `sonnet` | the drafting model: `opus`, `sonnet` or `haiku` |
| `KEA_ATTEMPTS` | `4` | drafts per check |
| `KEA_JOB_BUDGET_USD` | `1.5` | the most one check may spend |
| `KEA_JOB_RESERVE_USD` | `0.6` | what the daily budget keeps back for each running check |
| `KEA_DAILY_BUDGET_USD` | `5.0` | the most all checks may spend in a UTC day |
| `KEA_ASKS_PER_DAY` | `3` | checks per requester per UTC day |
| `KEA_MAX_QUEUE` | `30` | open jobs at once |
| `KEA_WATCH_MINUTES` | `15` | how often the watcher compares reports with live code |
| `KEA_ORIGINS` | `http://localhost:5173` | browser origins allowed to call the API, comma separated |
| `KEA_SITE` | `https://keaproofs.io` | the site the feeds link to |
| `KEA_TRUST_CF` | off | `1` reads the client from `CF-Connecting-IP`; set it only behind Cloudflare |
