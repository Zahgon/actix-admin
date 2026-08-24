use super::RoutePrelude;
use crate::admin_prelude;
use crate::prelude::*;
use axum::extract::{Path, RawQuery};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Form};
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tower_sessions::Session;

use super::helpers::run_local;
use super::query::ListQuery;

/// Delete file(s) attached to file-upload fields on the given model, best-effort.
fn delete_uploaded_files_for(
    actix_admin: &ActixAdmin,
    entity_name: &str,
    view_model: &ActixAdminViewModel,
    model: &ActixAdminModel,
) {
    for field in view_model.fields {
        if field.field_type != ActixAdminViewModelFieldType::FileUpload {
            continue;
        }
        let file_name = match model
            .get_value::<String>(&field.field_name, true, true)
            .ok()
            .flatten()
        {
            Some(name) if !name.is_empty() => name,
            _ => continue,
        };
        // Defensive: sanitize the DB-stored name too so a poisoned DB value
        // cannot escape the upload folder.
        let safe = crate::model::sanitize_upload_filename(&file_name);
        let file_path = format!(
            "{}/{}/{}",
            actix_admin.configuration.file_upload_directory, entity_name, safe
        );
        if let Err(e) = std::fs::remove_file(&file_path) {
            log::warn!("failed to remove uploaded file {file_path}: {e}");
        }
    }
}

pub async fn delete<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Extension(db): Extension<DatabaseConnection>,
    Path(id): Path<E::Id>,
) -> Result<Response, ActixAdminError> {
    run_local(delete_inner::<E>(
        session,
        headers,
        raw_query,
        actix_admin,
        db,
        id,
    ))
}

async fn delete_inner<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    raw_query: Option<String>,
    actix_admin: Arc<ActixAdmin>,
    db: DatabaseConnection,
    id: E::Id,
) -> Result<Response, ActixAdminError> {
    let actix_admin = &actix_admin;
    let raw_query = raw_query.unwrap_or_default();
    let ctx = admin_prelude!(
        &session,
        &headers,
        &raw_query,
        actix_admin,
        RoutePrelude::write(super::AdminAction::Delete),
        E
    );

    let db = &db;

    // Fetch first (to know upload paths) then delete.
    let model_result = E::get_entity(db, id.clone(), ctx.tenant_ref).await;
    let delete_result = E::delete_entity(db, id, ctx.tenant_ref).await;

    match (model_result, delete_result) {
        (Ok(model), Ok(_)) => {
            delete_uploaded_files_for(actix_admin, &ctx.entity_name, ctx.view_model, &model);
            Ok(StatusCode::OK.into_response())
        }
        (_, Err(e)) if e.ty == crate::ActixAdminErrorType::EntityDoesNotExistError => {
            Ok(StatusCode::NOT_FOUND.into_response())
        }
        (_, _) => Ok(StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    }
}

pub async fn delete_many<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Extension(db): Extension<DatabaseConnection>,
    Form(form): Form<Vec<(String, String)>>,
) -> Result<Response, ActixAdminError> {
    run_local(delete_many_inner::<E>(
        session,
        headers,
        raw_query,
        actix_admin,
        db,
        form,
    ))
}

async fn delete_many_inner<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    raw_query: Option<String>,
    actix_admin: Arc<ActixAdmin>,
    db: DatabaseConnection,
    form: Vec<(String, String)>,
) -> Result<Response, ActixAdminError> {
    let actix_admin = &actix_admin;
    let raw_query = raw_query.unwrap_or_default();
    let ctx = admin_prelude!(
        &session,
        &headers,
        &raw_query,
        actix_admin,
        RoutePrelude::write(super::AdminAction::Delete),
        E
    );

    let db = &db;
    let mut errors: Vec<crate::ActixAdminError> = Vec::new();

    // Silently skip un-parseable ids rather than panicking on client input.
    let ids: Vec<E::Id> = form
        .iter()
        .filter_map(|(k, v)| (k == "ids").then(|| v.parse::<E::Id>().ok()).flatten())
        .collect();

    // Pre-fetch models so we can delete their uploaded files after the DB
    // rows go away. This is best-effort: if a fetch fails the id is skipped.
    let mut fetched_models: Vec<ActixAdminModel> = Vec::with_capacity(ids.len());
    for id in &ids {
        match E::get_entity(db, id.clone(), ctx.tenant_ref).await {
            Ok(m) => fetched_models.push(m),
            Err(e) => errors.push(e),
        }
    }

    // Single batched DELETE ... WHERE pk IN (...).
    match E::delete_entities(db, &ids, ctx.tenant_ref).await {
        Ok(_) => {
            for model in &fetched_models {
                delete_uploaded_files_for(actix_admin, &ctx.entity_name, ctx.view_model, model);
            }
        }
        Err(e) => errors.push(e),
    }

    if errors.is_empty() {
        // Round-trip the pagination state that traveled in the form body
        // back into a URL query string, using the same encoder the list
        // route reads it with.
        let query = ListQuery::from_form(&form, ctx.view_model);
        Ok((
            StatusCode::SEE_OTHER,
            [(
                header::LOCATION,
                format!("list?{}", query.to_query_string()),
            )],
        )
            .into_response())
    } else {
        Ok(StatusCode::INTERNAL_SERVER_ERROR.into_response())
    }
}
