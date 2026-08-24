use super::helpers::{add_default_context_with_session, html_response, run_local, SearchParams};
use crate::admin_prelude;
use crate::prelude::*;
use crate::ActixAdminNotification;
use axum::extract::{Path, RawQuery};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::Extension;
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tera::Context;
use tower_sessions::Session;

use super::Params;
use super::{add_auth_context, render_template, RoutePrelude};

pub async fn show<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Extension(db): Extension<DatabaseConnection>,
    Path(id): Path<E::Id>,
) -> Result<Response, ActixAdminError> {
    run_local(show_inner::<E>(
        session,
        headers,
        raw_query,
        actix_admin,
        db,
        id,
    ))
}

async fn show_inner<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    raw_query: Option<String>,
    actix_admin: Arc<ActixAdmin>,
    db: DatabaseConnection,
    id: E::Id,
) -> Result<Response, ActixAdminError> {
    let actix_admin = &actix_admin;
    let raw_query = raw_query.unwrap_or_default();
    let ctx_data = admin_prelude!(
        &session,
        &headers,
        &raw_query,
        actix_admin,
        RoutePrelude::view(),
        E
    );

    let mut errors: Vec<crate::ActixAdminError> = Vec::new();
    let model = match E::get_entity(&db, id, ctx_data.tenant_ref).await {
        Ok(res) => res,
        Err(e) if e.ty == crate::ActixAdminErrorType::EntityDoesNotExistError => {
            // Short-circuit: don't try to render show.html with an empty model.
            let body = actix_admin
                .tera
                .render("not_found.html", &tera::Context::new())
                .unwrap_or_else(|_| String::from("Not Found"));
            return Ok(html_response(StatusCode::NOT_FOUND, body));
        }
        Err(e) => {
            errors.push(e);
            ActixAdminModel::create_empty()
        }
    };

    let http_response_code = match errors.first() {
        None => StatusCode::OK,
        Some(e) if e.ty == crate::ActixAdminErrorType::EntityDoesNotExistError => {
            StatusCode::NOT_FOUND
        }
        Some(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let notifications: Vec<ActixAdminNotification> = errors
        .into_iter()
        .map(ActixAdminNotification::from)
        .collect();

    let params = Params::from_query(&raw_query);
    let search_params = SearchParams::from_params(&params, ctx_data.view_model);

    let mut ctx = Context::new();
    add_auth_context(&session, actix_admin, &mut ctx).await;

    add_default_context_with_session(
        &mut ctx,
        &headers,
        ctx_data.view_model,
        ctx_data.entity_name,
        actix_admin,
        notifications,
        &search_params,
        Some(&session),
    );
    ctx.insert("model", &model);

    let body = render_template(&actix_admin.tera, "show.html", &ctx)
        .map_err(|e| ActixAdminError::internal(e.to_string()))?;
    Ok(html_response(http_response_code, body))
}
