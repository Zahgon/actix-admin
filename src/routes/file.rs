use crate::admin_prelude;
use crate::prelude::*;
use axum::body::Body;
use axum::extract::{Path, RawQuery};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tower::ServiceExt;
use tower_http::services::ServeFile;
use tower_sessions::Session;

use super::helpers::{html_response, run_local};
use super::{AdminAction, RoutePrelude};

/// Returns the field descriptor if `column_name` refers to a `FileUpload`
/// or `Image` field on the given view model. Rejects anything else to
/// prevent path traversal / disclosure through arbitrary column reads.
fn file_upload_field<'a>(
    view_model: &'a ActixAdminViewModel,
    column_name: &str,
) -> Result<&'a ActixAdminViewModelField, ActixAdminError> {
    view_model
        .fields
        .iter()
        .find(|f| {
            f.field_name == column_name
                && matches!(
                    f.field_type,
                    ActixAdminViewModelFieldType::FileUpload | ActixAdminViewModelFieldType::Image
                )
        })
        .ok_or_else(|| {
            ActixAdminError::bad_request(format!("'{column_name}' is not a file upload field"))
        })
}

pub async fn download<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Extension(db): Extension<DatabaseConnection>,
    Path((id, column_name)): Path<(E::Id, String)>,
) -> Result<Response, ActixAdminError> {
    run_local(download_inner::<E>(
        session,
        headers,
        raw_query,
        actix_admin,
        db,
        id,
        column_name,
    ))
}

async fn download_inner<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    raw_query: Option<String>,
    actix_admin: Arc<ActixAdmin>,
    db: DatabaseConnection,
    id: E::Id,
    column_name: String,
) -> Result<Response, ActixAdminError> {
    let actix_admin = &actix_admin;
    let db = &db;
    let raw_query = raw_query.unwrap_or_default();
    let ctx = admin_prelude!(
        &session,
        &headers,
        &raw_query,
        actix_admin,
        RoutePrelude::view(),
        E
    );

    let _field = file_upload_field(ctx.view_model, &column_name)?;

    let model = match E::get_entity(db, id, ctx.tenant_ref).await {
        Ok(m) => m,
        Err(e) if e.ty == crate::ActixAdminErrorType::EntityDoesNotExistError => {
            return Ok(StatusCode::NOT_FOUND.into_response());
        }
        Err(e) => return Err(ActixAdminError::internal(e.to_string())),
    };

    let file_name = model
        .get_value::<String>(&column_name, true, true)
        .ok()
        .flatten()
        .unwrap_or_default();
    if file_name.is_empty() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let safe = crate::model::sanitize_upload_filename(&file_name);
    let file_path = format!(
        "{}/{}/{}",
        actix_admin.configuration.file_upload_directory, ctx.entity_name, safe
    );

    // `ServeFile` serves the fixed path regardless of the request URI, but it
    // does read `Range` / `If-*` headers off the request, so the caller's
    // headers are forwarded to keep conditional and ranged downloads working
    // the way `NamedFile::into_response` did.
    let mut probe = Request::new(Body::empty());
    *probe.headers_mut() = headers.clone();
    let response = ServeFile::new(&file_path)
        .oneshot(probe)
        .await
        .map_err(|e| ActixAdminError::internal(e.to_string()))?;

    if response.status() == StatusCode::NOT_FOUND {
        return Ok(html_response(StatusCode::NOT_FOUND, String::new()));
    }
    Ok(response.into_response())
}

pub async fn delete_file<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Extension(db): Extension<DatabaseConnection>,
    Path((id, column_name)): Path<(E::Id, String)>,
) -> Result<Response, ActixAdminError> {
    run_local(delete_file_inner::<E>(
        session,
        headers,
        raw_query,
        actix_admin,
        db,
        id,
        column_name,
    ))
}

async fn delete_file_inner<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    raw_query: Option<String>,
    actix_admin: Arc<ActixAdmin>,
    db: DatabaseConnection,
    id: E::Id,
    column_name: String,
) -> Result<Response, ActixAdminError> {
    let actix_admin = &actix_admin;
    let raw_query = raw_query.unwrap_or_default();
    let ctx = admin_prelude!(
        &session,
        &headers,
        &raw_query,
        actix_admin,
        RoutePrelude::write(AdminAction::Edit),
        E
    );

    let view_model_field = file_upload_field(ctx.view_model, &column_name)?;

    let mut model = match E::get_entity(&db, id.clone(), ctx.tenant_ref).await {
        Ok(m) => m,
        Err(e) if e.ty == crate::ActixAdminErrorType::EntityDoesNotExistError => {
            return Ok(StatusCode::NOT_FOUND.into_response());
        }
        Err(e) => return Err(ActixAdminError::internal(e.to_string())),
    };

    if let Some(file_name) = model
        .get_value::<String>(&column_name, true, true)
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
    {
        let safe = crate::model::sanitize_upload_filename(&file_name);
        let file_path = format!(
            "{}/{}/{}",
            actix_admin.configuration.file_upload_directory, ctx.entity_name, safe
        );
        if let Err(e) = std::fs::remove_file(&file_path) {
            log::warn!("failed to remove uploaded file {file_path}: {e}");
        }
    }
    model.values.remove(&column_name);

    let _edit_res = E::edit_entity(&db, id, model.clone(), ctx.tenant_ref).await;

    let mut tctx = tera::Context::new();
    tctx.insert("model_field", view_model_field);
    tctx.insert("entity_name", &ctx.entity_name);
    tctx.insert("model", &model);

    let body = actix_admin
        .tera
        .render("form_elements/input.html", &tctx)
        .map_err(|e| ActixAdminError::internal(e.to_string()))?;
    Ok(html_response(StatusCode::OK, body))
}
