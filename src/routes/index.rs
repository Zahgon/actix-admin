use axum::http::StatusCode;
use axum::response::Response;
use axum::Extension;
use std::sync::Arc;
use tera::Context;
use tower_sessions::Session;

use crate::prelude::*;

use super::helpers::html_response;
use super::add_auth_context;

pub async fn get_admin_ctx(session: &Session, actix_admin: &ActixAdmin) -> Context {
    let mut ctx = Context::new();
    ctx.insert("entity_names", &actix_admin.entity_names);

    add_auth_context(session, actix_admin, &mut ctx).await;

    ctx
}

pub async fn index(
    session: Session,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
) -> Result<Response, ActixAdminError> {
    let actix_admin = &actix_admin;

    let mut ctx = Context::new();
    ctx.insert("entity_names", &actix_admin.entity_names);
    ctx.insert(
        "notifications",
        &Vec::<crate::ActixAdminNotification>::new(),
    );

    add_auth_context(&session, actix_admin, &mut ctx).await;

    let body = actix_admin
        .tera
        .render("index.html", &ctx)
        .map_err(|e| ActixAdminError::internal(format!("Template error: {e}")))?;
    Ok(html_response(StatusCode::OK, body))
}

pub async fn not_found(
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
) -> Result<Response, ActixAdminError> {
    let body = actix_admin
        .tera
        .render("not_found.html", &Context::new())
        .map_err(|e| ActixAdminError::internal(format!("Template error: {e}")))?;
    Ok(html_response(StatusCode::NOT_FOUND, body))
}
