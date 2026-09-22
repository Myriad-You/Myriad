//! Manifest and runtime Widget registry boundaries.

use super::{
    ApiResponse, MAX_WIDGETS_PER_TAPP, TappManifest, TappSettingDef, TappWidgetCategory,
    TappWidgetRefreshPolicy, current_is_admin, find_admin_user_id, lock_tapp_lifecycle,
    optional_authenticated_user_id, require_current_admin, validate_tapp_settings,
    validate_widget_refresh_policy,
};
use axum::{
    Extension, Json,
    extract::{Path, State},
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, EntityTrait, QueryFilter, QuerySelect, Set, Statement, TransactionTrait,
};
use serde::Deserialize;
use std::collections::HashSet;

use crate::api::tapp_runtime::RuntimeGrantContext;
use crate::api::tapp_runtime::common::parse_user_id;
use crate::middleware::auth::{Claims, OptionalClaims};
use crate::models::entities::{tapp_widgets, tapps};
use crate::services::permission_service::TappPermission;

use crate::error::HttpError;
use crate::services::tapp_lifecycle::{
    desired_manifest_widget_ids, format_tapp_widget_id, is_manifest_widget_row,
    legacy_manifest_widget_ids, local_widget_id_from_full, manifest_declares_local_widget_id,
    resolve_full_widget_id,
    runtime_widget_belongs_to_installation as runtime_widget_belongs_to_installation_domain,
    runtime_widget_register_shape_ok, runtime_widget_slot_available, widget_source,
};
use crate::services::tapp_ownership::public_install_visible_to_viewer;
use myriad_error::AppError;

// Domain ownership rules: services::tapp_lifecycle (path-stable Model adapter).
pub(super) fn runtime_widget_belongs_to_installation(
    widget: &tapp_widgets::Model,
    subject_id: i32,
    installation_owner_id: i32,
) -> bool {
    runtime_widget_belongs_to_installation_domain(&widget.config, subject_id, installation_owner_id)
}

fn tapp_widget_response(widget: &tapp_widgets::Model, is_admin_widget: bool) -> serde_json::Value {
    let local_id = local_widget_id_from_full(&widget.tapp_id, &widget.widget_id);
    serde_json::json!({
        "id": widget.widget_id,
        "tappId": widget.tapp_id,
        "config": {
            "id": local_id,
            "name": widget.name,
            "description": widget.description,
            "icon": widget.icon,
            "defaultSize": widget.default_size,
            "sizes": widget.sizes,
            "category": widget.category,
            "settings": widget.config.get("settings").cloned().unwrap_or_else(|| serde_json::json!([])),
            "refreshPolicy": widget.config.get("refreshPolicy").cloned().unwrap_or(serde_json::Value::Null),
        },
        "instanceCount": 0,
        "registeredAt": widget.registered_at.to_rfc3339(),
        "isAdminWidget": is_admin_widget,
    })
}

pub(super) async fn list_all_widgets(
    State(db): State<DatabaseConnection>,
    Extension(OptionalClaims(claims)): Extension<OptionalClaims>,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, HttpError> {
    let user_id = optional_authenticated_user_id(claims.as_ref());
    let is_admin = match claims.as_ref() {
        Some(claims) => current_is_admin(claims, &db).await,
        None => false,
    };
    let admin_id = find_admin_user_id(&db).await?;
    let admin_tapp_ids: HashSet<String> = if let Some(admin_id) = admin_id {
        tapps::Entity::find()
            .select_only()
            .columns([tapps::Column::TappId, tapps::Column::Visibility])
            .filter(tapps::Column::UserId.eq(admin_id))
            .into_tuple::<(String, String)>()
            .all(&db)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?
            .into_iter()
            .filter(|(_, visibility)| public_install_visible_to_viewer(visibility, is_admin))
            .map(|(id, _)| id)
            .collect()
    } else {
        HashSet::new()
    };
    let user_tapp_ids: HashSet<String> = if let Some(uid) = user_id {
        if Some(uid) != admin_id {
            tapps::Entity::find()
                .select_only()
                .column(tapps::Column::TappId)
                .filter(tapps::Column::UserId.eq(uid))
                .into_tuple::<String>()
                .all(&db)
                .await
                .map_err(|_| HttpError(AppError::internal("Database error")))?
                .into_iter()
                .collect()
        } else {
            HashSet::new()
        }
    } else {
        HashSet::new()
    };

    let mut items = Vec::new();
    let mut public_manifest_widget_ids = HashSet::new();
    let admin_widgets = if let Some(admin_id) = admin_id {
        tapp_widgets::Entity::find()
            .filter(tapp_widgets::Column::UserId.eq(admin_id))
            .all(&db)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?
    } else {
        Vec::new()
    };
    for widget in admin_widgets {
        // Hide widgets belonging to admin-only public installs for non-admins.
        if !admin_tapp_ids.contains(&widget.tapp_id)
            && widget_source(&widget.config) != Some("runtime")
        {
            continue;
        }
        if widget_source(&widget.config) == Some("runtime") {
            if user_id == admin_id
                && admin_id.is_some_and(|owner_id| {
                    runtime_widget_belongs_to_installation(&widget, owner_id, owner_id)
                })
            {
                items.push(tapp_widget_response(&widget, false));
            }
            continue;
        }
        if user_tapp_ids.contains(&widget.tapp_id) {
            continue;
        }
        public_manifest_widget_ids.insert(widget.widget_id.clone());
        items.push(tapp_widget_response(&widget, true));
    }

    if let Some(uid) = user_id {
        if Some(uid) != admin_id {
            let user_widgets = tapp_widgets::Entity::find()
                .filter(tapp_widgets::Column::UserId.eq(uid))
                .all(&db)
                .await
                .map_err(|_| HttpError(AppError::internal("Database error")))?;
            for widget in user_widgets {
                let visible = if user_tapp_ids.contains(&widget.tapp_id) {
                    widget_source(&widget.config) != Some("runtime")
                        || runtime_widget_belongs_to_installation(&widget, uid, uid)
                } else if admin_tapp_ids.contains(&widget.tapp_id) {
                    admin_id.is_some_and(|owner_id| {
                        runtime_widget_belongs_to_installation(&widget, uid, owner_id)
                            && !public_manifest_widget_ids.contains(&widget.widget_id)
                    })
                } else {
                    false
                };
                if visible {
                    items.push(tapp_widget_response(&widget, false));
                }
            }
        }
    }
    Ok(Json(ApiResponse::success(items)))
}

#[derive(Debug, Deserialize)]
pub struct RegisterWidgetRequest {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub default_size: String,
    pub sizes: Vec<String>,
    pub category: Option<TappWidgetCategory>,
    #[serde(default)]
    pub settings: Vec<TappSettingDef>,
    #[serde(default)]
    pub refresh_policy: Option<TappWidgetRefreshPolicy>,
}

pub(super) async fn reconcile_manifest_widgets(
    db: &impl ConnectionTrait,
    user_id: i32,
    tapp_id: &str,
    manifest: &TappManifest,
    previous_manifest: Option<&serde_json::Value>,
) -> Result<(), HttpError> {
    let desired_widgets = manifest.widgets.as_deref().unwrap_or_default();
    let desired_ids =
        desired_manifest_widget_ids(tapp_id, desired_widgets.iter().map(|w| w.id.as_str()));
    let legacy_manifest_ids = legacy_manifest_widget_ids(tapp_id, previous_manifest);

    let existing = tapp_widgets::Entity::find()
        .filter(tapp_widgets::Column::UserId.eq(user_id))
        .filter(tapp_widgets::Column::TappId.eq(tapp_id))
        .all(db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
    for widget in existing {
        if is_manifest_widget_row(&widget.config, &widget.widget_id, &legacy_manifest_ids)
            && !desired_ids.contains(&widget.widget_id)
        {
            tapp_widgets::Entity::delete_by_id(widget.id)
                .exec(db)
                .await
                .map_err(|_| HttpError(AppError::internal("Database error")))?;
        }
    }

    for widget in desired_widgets {
        let widget_id = format_tapp_widget_id(tapp_id, &widget.id);
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"DELETE FROM tapp_widgets
               WHERE widget_id = $1
                 AND user_id <> $2
                 AND config->>'source' = 'runtime'
                 AND config->>'installationOwnerId' = $3"#,
            vec![
                widget_id.clone().into(),
                user_id.into(),
                user_id.to_string().into(),
            ],
        ))
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
        let runtime_config = serde_json::json!({
            "settings": &widget.settings,
            "refreshPolicy": &widget.refresh_policy,
            "source": "manifest",
            "installationOwnerId": user_id,
        });
        let existing = tapp_widgets::Entity::find()
            .filter(tapp_widgets::Column::UserId.eq(user_id))
            .filter(tapp_widgets::Column::WidgetId.eq(&widget_id))
            .one(db)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?;
        if let Some(existing) = existing {
            let mut active: tapp_widgets::ActiveModel = existing.into();
            active.name = Set(widget.name.clone());
            active.description = Set(widget.description.clone());
            active.icon = Set(widget.icon.clone());
            active.default_size = Set(widget.default_size.clone());
            active.sizes = Set(serde_json::to_value(&widget.sizes).unwrap_or_default());
            active.category = Set(widget
                .category
                .map(|category| category.as_str().to_string()));
            active.config = Set(runtime_config);
            active
                .update(db)
                .await
                .map_err(|_| HttpError(AppError::internal("Database error")))?;
        } else {
            tapp_widgets::ActiveModel {
                id: NotSet,
                widget_id: Set(widget_id),
                tapp_id: Set(tapp_id.to_string()),
                user_id: Set(user_id),
                name: Set(widget.name.clone()),
                description: Set(widget.description.clone()),
                icon: Set(widget.icon.clone()),
                default_size: Set(widget.default_size.clone()),
                sizes: Set(serde_json::to_value(&widget.sizes).unwrap_or_default()),
                category: Set(widget
                    .category
                    .map(|category| category.as_str().to_string())),
                config: Set(runtime_config),
                registered_at: Set(Utc::now().fixed_offset()),
            }
            .insert(db)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?;
        }
    }
    Ok(())
}

pub(super) async fn register_widget(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
    Json(request): Json<RegisterWidgetRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    require_current_admin(&claims, &db).await?;
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::WidgetRegister)?;
    let user_id = parse_user_id(&claims)?;
    let installation_owner_id = runtime_grant.owner_id();
    if !runtime_widget_register_shape_ok(
        &request.id,
        &request.name,
        &request.sizes,
        &request.default_size,
    ) {
        return Err(HttpError(AppError::bad_request("Bad request")));
    }
    validate_tapp_settings(&request.settings, &format!("Widget {}", request.id))
        .map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    if let Some(policy) = &request.refresh_policy {
        validate_widget_refresh_policy(policy, &request.id)
            .map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    }

    let widget_id = format_tapp_widget_id(&tapp_id, &request.id);
    let site_owner_id = find_admin_user_id(&db).await?;
    let txn = db
        .begin()
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
    lock_tapp_lifecycle(&txn, &tapp_id)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
    let private_install_exists = if Some(user_id) != site_owner_id {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(user_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&txn)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?
            .is_some()
    } else {
        false
    };
    let public_install_exists = if let Some(site_owner_id) = site_owner_id {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(site_owner_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&txn)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?
            .is_some()
    } else {
        false
    };
    let visible_owner_id = if private_install_exists {
        user_id
    } else if public_install_exists {
        site_owner_id.ok_or_else(|| HttpError(AppError::internal("Database error")))?
    } else {
        user_id
    };
    if visible_owner_id != installation_owner_id {
        txn.rollback().await.ok();
        return Err(HttpError(AppError::forbidden("Forbidden")));
    }

    let installed_tapp = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(installation_owner_id))
        .filter(tapps::Column::TappId.eq(&tapp_id))
        .one(&txn)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?
        .ok_or_else(|| HttpError(AppError::forbidden("Forbidden")))?;
    if manifest_declares_local_widget_id(&installed_tapp.manifest, &request.id) {
        return Err(HttpError(AppError::conflict("Conflict")));
    }
    let runtime_config = serde_json::json!({
        "settings": &request.settings,
        "refreshPolicy": &request.refresh_policy,
        "source": "runtime",
        "installationOwnerId": installation_owner_id,
    });
    let now = Utc::now().fixed_offset();
    let existing = tapp_widgets::Entity::find()
        .filter(tapp_widgets::Column::UserId.eq(user_id))
        .filter(tapp_widgets::Column::WidgetId.eq(&widget_id))
        .one(&txn)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
    if let Some(item) = existing {
        if item
            .config
            .get("source")
            .and_then(serde_json::Value::as_str)
            == Some("manifest")
        {
            return Err(HttpError(AppError::conflict("Conflict")));
        }
        if !runtime_widget_belongs_to_installation(&item, user_id, installation_owner_id) {
            return Err(HttpError(AppError::conflict("Conflict")));
        }
        let mut active: tapp_widgets::ActiveModel = item.into();
        active.name = Set(request.name.clone());
        active.description = Set(request.description.clone());
        active.icon = Set(request.icon.clone());
        active.default_size = Set(request.default_size.clone());
        active.sizes = Set(serde_json::to_value(&request.sizes).unwrap());
        active.category = Set(request
            .category
            .map(|category| category.as_str().to_string()));
        active.config = Set(runtime_config.clone());
        active
            .update(&txn)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?;
    } else {
        let manifest_widget_count = installed_tapp
            .manifest
            .get("widgets")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len);
        let subject_widgets = tapp_widgets::Entity::find()
            .filter(tapp_widgets::Column::UserId.eq(user_id))
            .filter(tapp_widgets::Column::TappId.eq(&tapp_id))
            .all(&txn)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?;
        let runtime_widget_count = subject_widgets
            .iter()
            .filter(|widget| {
                runtime_widget_belongs_to_installation(widget, user_id, installation_owner_id)
            })
            .count();
        if !runtime_widget_slot_available(
            manifest_widget_count,
            runtime_widget_count,
            MAX_WIDGETS_PER_TAPP,
        ) {
            return Err(HttpError(AppError::bad_request("Bad request")));
        }
        tapp_widgets::ActiveModel {
            id: NotSet,
            widget_id: Set(widget_id.clone()),
            tapp_id: Set(tapp_id.clone()),
            user_id: Set(user_id),
            name: Set(request.name.clone()),
            description: Set(request.description.clone()),
            icon: Set(request.icon.clone()),
            default_size: Set(request.default_size.clone()),
            sizes: Set(serde_json::to_value(&request.sizes).unwrap()),
            category: Set(request
                .category
                .map(|category| category.as_str().to_string())),
            config: Set(runtime_config),
            registered_at: Set(now),
        }
        .insert(&txn)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
    }
    txn.commit()
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
    Ok(Json(ApiResponse::success(serde_json::json!({
        "id": widget_id,
        "tappId": tapp_id,
        "name": request.name,
    }))))
}

pub(super) async fn unregister_widget(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, widget_id)): Path<(String, String)>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    require_current_admin(&claims, &db).await?;
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::WidgetRegister)?;
    let user_id = parse_user_id(&claims)?;
    let installation_owner_id = runtime_grant.owner_id();
    let full_widget_id = resolve_full_widget_id(&tapp_id, &widget_id)
        .map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let widget = tapp_widgets::Entity::find()
        .filter(tapp_widgets::Column::UserId.eq(user_id))
        .filter(tapp_widgets::Column::WidgetId.eq(&full_widget_id))
        .one(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
    if let Some(widget) = &widget {
        if widget_source(&widget.config) == Some("manifest") {
            return Err(HttpError(AppError::conflict("Conflict")));
        }
        if !runtime_widget_belongs_to_installation(widget, user_id, installation_owner_id) {
            return Err(HttpError(AppError::not_found("Not found")));
        }
    }
    tapp_widgets::Entity::delete_many()
        .filter(tapp_widgets::Column::UserId.eq(user_id))
        .filter(tapp_widgets::Column::WidgetId.eq(&full_widget_id))
        .exec(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
    Ok(Json(ApiResponse::success(())))
}
