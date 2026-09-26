use crate::services::tapp_lifecycle::{format_tapp_widget_id, legacy_manifest_widget_ids};
use myriad_error::AppError;
use myriad_tapp_contract::manifest::{TappManifest, TappWidgetDef};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value as SeaValue};
use std::collections::HashSet;
/// `$first, $first+1, …` for `count` bind parameters.
fn placeholder_list(first: usize, count: usize) -> String {
    (first..first + count)
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reconcile manifest widget rows with a fixed number of set statements:
/// one stale-manifest DELETE, one runtime-conflict DELETE and one multi-row
/// upsert on `(user_id, tapp_id, widget_id)`. Callers own the lifecycle
/// transaction; any failed statement rolls the whole install/update back.
pub(crate) async fn reconcile_manifest_widgets(
    db: &impl ConnectionTrait,
    user_id: i32,
    tapp_id: &str,
    manifest: &TappManifest,
    previous_manifest: Option<&serde_json::Value>,
) -> Result<(), AppError> {
    let db_error = |_| AppError::internal("Database error");
    let desired_widgets = manifest.widgets.as_deref().unwrap_or_default();
    // Manifest validation rejects duplicate ids; keep the last one regardless so
    // the upsert never touches the same row twice.
    let mut seen = HashSet::new();
    let mut desired: Vec<(String, &TappWidgetDef)> = desired_widgets
        .iter()
        .rev()
        .map(|widget| (format_tapp_widget_id(tapp_id, &widget.id), widget))
        .filter(|(widget_id, _)| seen.insert(widget_id.clone()))
        .collect();
    desired.reverse();
    let legacy_manifest_ids = legacy_manifest_widget_ids(tapp_id, previous_manifest);

    // Stale manifest rows (`config.source = manifest` or a legacy source-less
    // manifest id) that the new manifest no longer declares.
    let mut values: Vec<SeaValue> = vec![user_id.into(), tapp_id.into()];
    let mut manifest_row = "config->>'source' = 'manifest'".to_string();
    if !legacy_manifest_ids.is_empty() {
        manifest_row.push_str(&format!(
            " OR widget_id IN ({})",
            placeholder_list(values.len() + 1, legacy_manifest_ids.len())
        ));
        values.extend(
            legacy_manifest_ids
                .iter()
                .map(|id| SeaValue::from(id.as_str())),
        );
    }
    let mut sql = format!(
        "DELETE FROM tapp_widgets WHERE user_id = $1 AND tapp_id = $2 AND ({manifest_row})"
    );
    if !desired.is_empty() {
        sql.push_str(&format!(
            " AND widget_id NOT IN ({})",
            placeholder_list(values.len() + 1, desired.len())
        ));
        values.extend(desired.iter().map(|(id, _)| SeaValue::from(id.as_str())));
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        values,
    ))
    .await
    .map_err(db_error)?;
    if desired.is_empty() {
        return Ok(());
    }

    // Runtime rows another user registered for this owner's installation.
    let mut values: Vec<SeaValue> = vec![user_id.into(), user_id.to_string().into()];
    values.extend(desired.iter().map(|(id, _)| SeaValue::from(id.as_str())));
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        format!(
            r#"DELETE FROM tapp_widgets
               WHERE user_id <> $1
                 AND config->>'source' = 'runtime'
                 AND config->>'installationOwnerId' = $2
                 AND widget_id IN ({})"#,
            placeholder_list(3, desired.len())
        ),
        values,
    ))
    .await
    .map_err(db_error)?;

    const COLUMNS: usize = 10;
    let mut values: Vec<SeaValue> = Vec::with_capacity(desired.len() * COLUMNS);
    let mut rows = Vec::with_capacity(desired.len());
    for (widget_id, widget) in desired {
        let first = values.len() + 1;
        rows.push(format!("({}, NOW())", placeholder_list(first, COLUMNS)));
        let runtime_config = serde_json::json!({
            "settings": &widget.settings,
            "refreshPolicy": &widget.refresh_policy,
            "source": "manifest",
            "installationOwnerId": user_id,
        });
        values.extend([
            widget_id.into(),
            tapp_id.into(),
            user_id.into(),
            widget.name.as_str().into(),
            widget.description.clone().into(),
            widget.icon.clone().into(),
            widget.default_size.as_str().into(),
            serde_json::to_value(&widget.sizes)
                .unwrap_or_default()
                .into(),
            widget
                .category
                .map(|category| category.as_str().to_string())
                .into(),
            runtime_config.into(),
        ]);
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        format!(
            "INSERT INTO tapp_widgets (widget_id, tapp_id, user_id, name, description, icon, \
             default_size, sizes, category, config, registered_at) VALUES {} \
             ON CONFLICT (user_id, tapp_id, widget_id) DO UPDATE SET \
             name = EXCLUDED.name, description = EXCLUDED.description, icon = EXCLUDED.icon, \
             default_size = EXCLUDED.default_size, sizes = EXCLUDED.sizes, \
             category = EXCLUDED.category, config = EXCLUDED.config",
            rows.join(", ")
        ),
        values,
    ))
    .await
    .map_err(db_error)?;
    Ok(())
}
