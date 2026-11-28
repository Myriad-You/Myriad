//! 开一个任务：领下它（`claim_for`），再在它自己的任务里从任务记录跑——归谁、从哪张图做、
//! 做成什么都从记录读回（`run_recorded`），生图和拆层服务用站点配置的。

use super::*;

/// The site lineage an archive of the job keeps: the full-body set it was
/// made for, and its master's generation contract.
#[derive(Default)]
pub(super) struct Lineage {
    pub(super) outfit_id: Option<String>,
    pub(super) fingerprint: Option<String>,
}

/// A job of a picture claimed as `claim` has it and, when it is new, set
/// running on its own task from what its record keeps (`run_recorded`).
/// Its caller has read the picture the input names before, so that a
/// picture that cannot be had refuses the start.
pub(super) async fn start_claimed(
    db: DatabaseConnection,
    claim: Claim,
    providers: Providers,
    lineage: Lineage,
) -> ApiResult<(String, bool)> {
    let JobInput::Picture { picture, make } = claim.input.clone() else {
        return Err(internal_error("a kept job is not started from a picture"));
    };
    let (stage, total) = make.first_stage();
    let targets = live::Targets {
        space: Some(providers.space_name.clone()),
        image: Some(format!(
            "{}/{}",
            providers.generation.provider, providers.generation.model
        )),
    };
    let (id, new) = claim_for(&db, claim, make.phase(), stage, total, targets).await?;
    if !new {
        return Ok((id, false));
    }
    tracing::info!(job = %id, outfit = ?lineage.outfit_id, "turn keyforms started");
    let meta = ArchiveMeta {
        id: archive::new_id(),
        created_at: crate::clock::now(),
        outfit_id: lineage.outfit_id,
        source_master_asset_id: picture.name(),
        source_generation_fingerprint: lineage.fingerprint,
        parent: None,
        master: String::new(),
        master_media_type: String::new(),
        generation_provider: providers.generation.provider.clone(),
        generation_model: providers.generation.model.clone(),
        prompts: turn_prompts().to_vec(),
        space: providers.space_name.clone(),
        redrawn: Vec::new(),
        bytes: 0,
        figure: None,
        body_motions: None,
    };
    let task_id = id.clone();
    tokio::spawn(async move {
        let work = run_recorded(db.clone(), task_id.clone(), providers);
        live::run_job(db.clone(), task_id.clone(), work, meta).await;
    });
    Ok((id, true))
}

/// A claimed job of a picture run from its record alone: whose it is (the
/// storage its cache and files go to) and its input (the picture, read from
/// where the input names it, and what is made of it); `providers` are the
/// site's, never kept on the record.
pub(super) async fn run_recorded(
    db: DatabaseConnection,
    id: String,
    providers: Providers,
) -> ApiResult<Made> {
    let (owner, input) = recorded(&db, &id).await?;
    let JobInput::Picture { picture, make } = input else {
        return Err(internal_error("a kept job is not run from a picture"));
    };
    let storage = owner.storage();
    let reference = load_picture(&picture).await?;
    let Providers {
        generation,
        client,
        space_name,
    } = providers;
    let cache = TurnCache::for_master(
        &storage,
        &reference.bytes,
        &generation,
        &space_name,
        see_through::DecomposeOptions::default(),
    );
    // Each way of making it is a large future in a debug build: kept on the
    // heap, not on the stack of the thread that polls it.
    let result = match make {
        Make::Figure { head, body } => {
            let keys = FigureKeys { head, body };
            Box::pin(run_figure(
                &id,
                &storage,
                &generation,
                reference,
                &client,
                &space_name,
                keys,
            ))
            .await
        }
        Make::Plain => Box::pin(run_plain(&id, reference, &client, &cache)).await,
        Make::Bust => {
            let alone = HeadSplit::unwatched();
            Box::pin(run(&id, &generation, reference, &client, &cache, alone)).await
        }
    };
    prune_cache(cache_root(&storage), CACHE_KEEP).await;
    result
}

/// How often reading a job's record back is tried before its run gives up.
const RECORD_READS: usize = 3;

/// Whose a job is and what its steps run from, as its record keeps them.
pub(super) async fn recorded(
    db: &DatabaseConnection,
    id: &str,
) -> ApiResult<(rig_jobs::Owner, JobInput)> {
    let mut tries = 0;
    let row = loop {
        tries += 1;
        match rig_jobs::get(db, id).await {
            Ok(row) => break row,
            Err(error) if tries < RECORD_READS => {
                tracing::warn!(%error, job = %id, "turn keyforms record could not be read; trying again");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(error) => return Err(internal_error(error)),
        }
    };
    let row = row.ok_or_else(missing_job)?;
    let owner = row
        .owner()
        .ok_or_else(|| internal_error("the job's record names no owner this build knows"))?;
    let input = row
        .pipeline_input
        .ok_or_else(|| internal_error("the job's record keeps no input"))
        .and_then(|input| serde_json::from_value(input).map_err(internal_error))?;
    Ok((owner, input))
}
