use crate::admin_prelude;
use crate::prelude::*;
use axum::extract::{Path, RawQuery};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::Extension;
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tower_sessions::Session;

use super::helpers::{html_response, run_local};
use super::{render_create_or_edit_form, RoutePrelude};

pub async fn create_get<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Extension(db): Extension<DatabaseConnection>,
) -> Result<Response, ActixAdminError> {
    run_local(create_get_inner::<E>(
        session,
        headers,
        raw_query,
        actix_admin,
        db,
    ))
}

async fn create_get_inner<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    raw_query: Option<String>,
    actix_admin: Arc<ActixAdmin>,
    db: DatabaseConnection,
) -> Result<Response, ActixAdminError> {
    let actix_admin = &actix_admin;
    let raw_query = raw_query.unwrap_or_default();
    let ctx = admin_prelude!(
        &session,
        &headers,
        &raw_query,
        actix_admin,
        RoutePrelude::create(),
        E
    );

    render_create_or_edit_form::<E>(
        &session,
        &headers,
        &raw_query,
        actix_admin,
        ctx.view_model,
        &db,
        ctx.entity_name,
        &ActixAdminModel::create_empty(),
        ctx.tenant_ref,
        Vec::new(),
        false,
        StatusCode::OK,
    )
    .await
}

pub async fn edit_get<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Extension(db): Extension<DatabaseConnection>,
    Path(id): Path<E::Id>,
) -> Result<Response, ActixAdminError> {
    run_local(edit_get_inner::<E>(
        session,
        headers,
        raw_query,
        actix_admin,
        db,
        id,
    ))
}

async fn edit_get_inner<E: ActixAdminViewModelTrait>(
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
        RoutePrelude::edit(),
        E
    );

    let db = &db;
    let model_result = E::get_entity(db, id, ctx.tenant_ref).await;

    let (model, notifications, status) = match model_result {
        Ok(m) => (m, Vec::new(), StatusCode::OK),
        Err(e) if e.ty == crate::ActixAdminErrorType::EntityDoesNotExistError => {
            let body = actix_admin
                .tera
                .render("not_found.html", &tera::Context::new())
                .unwrap_or_else(|_| String::from("Not Found"));
            return Ok(html_response(StatusCode::NOT_FOUND, body));
        }
        Err(e) => (
            ActixAdminModel::create_empty(),
            vec![crate::ActixAdminNotification::from(e)],
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    };

    render_create_or_edit_form::<E>(
        &session,
        &headers,
        &raw_query,
        actix_admin,
        ctx.view_model,
        db,
        ctx.entity_name,
        &model,
        ctx.tenant_ref,
        notifications,
        ctx.view_model.inline_edit,
        status,
    )
    .await
}
