//! Manifest and runtime Widget registry boundaries.

use super::{
    authorize_tapp_permission, find_admin_user_id, is_safe_path_component, is_valid_widget_size,
    lock_tapp_lifecycle, optional_authenticated_user_id, require_current_admin,
    validate_tapp_settings, validate_widget_refresh_policy, ApiResponse, TappManifest,
    TappSettingDef, TappWidgetCategory, TappWidgetRefreshPolicy, MAX_WIDGETS_PER_TAPP,
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Extension, Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, EntityTrait, QueryFilter, Set, Statement, TransactionTrait,
};
use serde::Deserialize;
use std::collections::HashSet;

use crate::api::tapp_runtime::RuntimeGrantContext;
use crate::middleware::auth::{extract_optional_claims, Claims};
use crate::models::entities::{tapp_widgets, tapps};
use crate::services::permission_service::TappPermission;

fn widget_source(config: &serde_json::Value) -> Option<&str> {
    config.get("source").and_then(serde_json::Value::as_str)
}

fn widget_installation_owner(config: &serde_json::Value) -> Option<i32> {
    config
        .get("installationOwnerId")
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

pub(super) fn runtime_widget_belongs_to_installation(
    widget: &tapp_widgets::Model,
    subject_id: i32,
    installation_owner_id: i32,
) -> bool {
    widget_source(&widget.config) == Some("runtime")
        && (widget_installation_owner(&widget.config) == Some(installation_owner_id)
            || (widget_installation_owner(&widget.config).is_none()
                && subject_id == installation_owner_id))
}

fn tapp_widget_response(widget: &tapp_widgets::Model, is_admin_widget: bool) -> serde_json::Value {
    serde_json::json!({
        "id": widget.widget_id,
        "tappId": widget.tapp_id,
        "config": {
            "id": widget.widget_id.strip_prefix(&format!("tapp.{}.", widget.tapp_id)).unwrap_or(&widget.widget_id),
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
    headers: HeaderMap,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, StatusCode> {
    let claims = extract_optional_claims(&headers);
    let user_id = optional_authenticated_user_id(claims.as_ref());
    let admin_id = find_admin_user_id(&db).await?;
    let admin_tapp_ids: HashSet<String> = if let Some(admin_id) = admin_id {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(admin_id))
            .all(&db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .into_iter()
            .map(|tapp| tapp.tapp_id)
            .collect()
    } else {
        HashSet::new()
    };
    let user_tapp_ids: HashSet<String> = if let Some(uid) = user_id {
        if Some(uid) != admin_id {
            tapps::Entity::find()
                .filter(tapps::Column::UserId.eq(uid))
                .all(&db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .into_iter()
                .map(|tapp| tapp.tapp_id)
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
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        Vec::new()
    };
    for widget in admin_widgets {
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
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
) -> Result<(), StatusCode> {
    let desired_widgets = manifest.widgets.as_deref().unwrap_or_default();
    let desired_ids: HashSet<String> = desired_widgets
        .iter()
        .map(|widget| format!("tapp.{tapp_id}.{}", widget.id))
        .collect();
    let legacy_manifest_ids: HashSet<String> = previous_manifest
        .and_then(|value| value.get("widgets"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|widget| widget.get("id").and_then(serde_json::Value::as_str))
        .map(|id| format!("tapp.{tapp_id}.{id}"))
        .collect();

    let existing = tapp_widgets::Entity::find()
        .filter(tapp_widgets::Column::UserId.eq(user_id))
        .filter(tapp_widgets::Column::TappId.eq(tapp_id))
        .all(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    for widget in existing {
        let is_manifest = widget
            .config
            .get("source")
            .and_then(serde_json::Value::as_str)
            == Some("manifest")
            || legacy_manifest_ids.contains(&widget.widget_id);
        if is_manifest && !desired_ids.contains(&widget.widget_id) {
            tapp_widgets::Entity::delete_by_id(widget.id)
                .exec(db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
    }

    for widget in desired_widgets {
        let widget_id = format!("tapp.{tapp_id}.{}", widget.id);
        db.execute(Statement::from_sql_and_values(
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
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
) -> Result<Json<ApiResponse<serde_json::Value>>, StatusCode> {
    require_current_admin(&claims).await?;
    runtime_grant
        .require_tapp_id(&tapp_id)
        .and_then(|_| runtime_grant.require(TappPermission::WidgetRegister))
        .map_err(|(status, _)| status)?;
    let user_id =
        authorize_tapp_permission(&db, &claims, &tapp_id, TappPermission::WidgetRegister).await?;
    let installation_owner_id = runtime_grant.owner_id();
    if !is_safe_path_component(&request.id)
        || request.name.is_empty()
        || request.name.len() > 255
        || request.sizes.is_empty()
        || request.sizes.len() > 10
        || request.sizes.iter().any(|size| !is_valid_widget_size(size))
        || !request.sizes.contains(&request.default_size)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    validate_tapp_settings(&request.settings, &format!("Widget {}", request.id))
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if let Some(policy) = &request.refresh_policy {
        validate_widget_refresh_policy(policy, &request.id).map_err(|_| StatusCode::BAD_REQUEST)?;
    }

    let widget_id = format!("tapp.{}.{}", tapp_id, request.id);
    let site_owner_id = find_admin_user_id(&db).await?;
    let txn = db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    lock_tapp_lifecycle(&txn, &tapp_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let private_install_exists = if Some(user_id) != site_owner_id {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(user_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&txn)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
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
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .is_some()
    } else {
        false
    };
    let visible_owner_id = if private_install_exists {
        user_id
    } else if public_install_exists {
        site_owner_id.ok_or(StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        user_id
    };
    if visible_owner_id != installation_owner_id {
        txn.rollback().await.ok();
        return Err(StatusCode::FORBIDDEN);
    }

    let installed_tapp = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(installation_owner_id))
        .filter(tapps::Column::TappId.eq(&tapp_id))
        .one(&txn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::FORBIDDEN)?;
    if installed_tapp
        .manifest
        .get("widgets")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|widgets| {
            widgets.iter().any(|widget| {
                widget.get("id").and_then(serde_json::Value::as_str) == Some(request.id.as_str())
            })
        })
    {
        return Err(StatusCode::CONFLICT);
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
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(item) = existing {
        if item
            .config
            .get("source")
            .and_then(serde_json::Value::as_str)
            == Some("manifest")
        {
            return Err(StatusCode::CONFLICT);
        }
        if !runtime_widget_belongs_to_installation(&item, user_id, installation_owner_id) {
            return Err(StatusCode::CONFLICT);
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
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let runtime_widget_count = subject_widgets
            .iter()
            .filter(|widget| {
                runtime_widget_belongs_to_installation(widget, user_id, installation_owner_id)
            })
            .count();
        if manifest_widget_count + runtime_widget_count >= MAX_WIDGETS_PER_TAPP {
            return Err(StatusCode::BAD_REQUEST);
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
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    txn.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    require_current_admin(&claims).await?;
    runtime_grant
        .require_tapp_id(&tapp_id)
        .and_then(|_| runtime_grant.require(TappPermission::WidgetRegister))
        .map_err(|(status, _)| status)?;
    let user_id =
        authorize_tapp_permission(&db, &claims, &tapp_id, TappPermission::WidgetRegister).await?;
    let installation_owner_id = runtime_grant.owner_id();
    let full_widget_id = if widget_id.starts_with("tapp.") {
        let expected_prefix = format!("tapp.{}.", tapp_id);
        if !widget_id.starts_with(&expected_prefix) {
            return Err(StatusCode::BAD_REQUEST);
        }
        widget_id
    } else {
        format!("tapp.{}.{}", tapp_id, widget_id)
    };
    let widget = tapp_widgets::Entity::find()
        .filter(tapp_widgets::Column::UserId.eq(user_id))
        .filter(tapp_widgets::Column::WidgetId.eq(&full_widget_id))
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(widget) = &widget {
        if widget_source(&widget.config) == Some("manifest") {
            return Err(StatusCode::CONFLICT);
        }
        if !runtime_widget_belongs_to_installation(widget, user_id, installation_owner_id) {
            return Err(StatusCode::NOT_FOUND);
        }
    }
    tapp_widgets::Entity::delete_many()
        .filter(tapp_widgets::Column::UserId.eq(user_id))
        .filter(tapp_widgets::Column::WidgetId.eq(&full_widget_id))
        .exec(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ApiResponse::success(())))
}
