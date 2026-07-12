use anyhow::{anyhow, Context, Result};
use divine_atbridge::legacy_repair::{LegacyRepairService, BADJWT_SIGNATURE_ERROR};
use divine_bridge_db::LegacyBadJwtRepairFilter;

const HELP: &str = "Usage:
  repair-legacy-badjwt --actor ACTOR --nostr-pubkey HEX [--event-id HEX ... | --exact-badjwt]
                       [--after-event-id HEX] [--max-rows N]
  repair-legacy-badjwt --operation-id UUID --confirm-digest SHA256
  repair-legacy-badjwt --rollback-operation-id UUID

Preview is the default and never changes publish jobs. Confirmation requires the
operation ID and digest printed by preview. DATABASE_URL is read only after
arguments have been validated.";

#[derive(Default)]
struct Args {
    actor: Option<String>,
    nostr_pubkey: Option<String>,
    event_ids: Vec<String>,
    exact_badjwt: bool,
    after_event_id: Option<String>,
    max_rows: i64,
    operation_id: Option<String>,
    confirm_digest: Option<String>,
    rollback_operation_id: Option<String>,
}

fn main() -> Result<()> {
    let raw = std::env::args().skip(1).collect::<Vec<_>>();
    if raw.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{HELP}");
        return Ok(());
    }
    let args = parse_args(&raw)?;
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let service = LegacyRepairService::new(database_url);

    if let Some(operation_id) = args.rollback_operation_id.as_deref() {
        if args.operation_id.is_some() || args.confirm_digest.is_some() {
            return Err(anyhow!("rollback cannot be combined with confirmation"));
        }
        let result = service.rollback(operation_id)?;
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }

    if let (Some(operation_id), Some(digest)) =
        (args.operation_id.as_deref(), args.confirm_digest.as_deref())
    {
        let result = service.confirm(operation_id, digest)?;
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }
    if args.operation_id.is_some() || args.confirm_digest.is_some() {
        return Err(anyhow!(
            "--operation-id and --confirm-digest must be supplied together"
        ));
    }

    let actor = args
        .actor
        .as_deref()
        .ok_or_else(|| anyhow!("--actor is required"))?;
    let nostr_pubkey = args
        .nostr_pubkey
        .ok_or_else(|| anyhow!("--nostr-pubkey is required"))?;
    let result = service.preview(
        actor,
        LegacyBadJwtRepairFilter {
            nostr_pubkey,
            event_ids: args.event_ids,
            exact_error: args
                .exact_badjwt
                .then(|| BADJWT_SIGNATURE_ERROR.to_string()),
            after_event_id: args.after_event_id,
            limit: args.max_rows,
        },
    )?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

fn parse_args(raw: &[String]) -> Result<Args> {
    let mut parsed = Args {
        max_rows: 100,
        ..Args::default()
    };
    let mut index = 0;
    while index < raw.len() {
        let flag = raw[index].as_str();
        if flag == "--exact-badjwt" {
            parsed.exact_badjwt = true;
            index += 1;
            continue;
        }
        let value = raw
            .get(index + 1)
            .ok_or_else(|| anyhow!("missing value for option"))?
            .clone();
        match flag {
            "--actor" => parsed.actor = Some(value),
            "--nostr-pubkey" => parsed.nostr_pubkey = Some(value),
            "--event-id" => parsed.event_ids.push(value),
            "--after-event-id" => parsed.after_event_id = Some(value),
            "--max-rows" => {
                parsed.max_rows = value.parse().context("--max-rows must be an integer")?
            }
            "--operation-id" => parsed.operation_id = Some(value),
            "--confirm-digest" => parsed.confirm_digest = Some(value),
            "--rollback-operation-id" => parsed.rollback_operation_id = Some(value),
            _ => return Err(anyhow!("unknown option")),
        }
        index += 2;
    }
    Ok(parsed)
}
