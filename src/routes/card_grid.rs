use axum::extract::OriginalUri;
use axum::http::StatusCode;
use axum::response::Response;
use axum::Extension;
use std::sync::Arc;
use tera::Context;
use tower_sessions::Session;

use crate::prelude::*;

use super::helpers::html_response;
use super::add_auth_context;

pub async fn display_card_grid(
    session: Session,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    OriginalUri(uri): OriginalUri,
) -> Result<Response, ActixAdminError> {
    let actix_admin = &actix_admin;
    // `OriginalUri` (not the extractor `Uri`) because `Router::nest` strips
    // the `base_path` prefix from the request URI, and this lookup key is
    // derived by removing that same prefix from the full path.
    let path = uri
        .path()
        .replace(actix_admin.configuration.base_path, "")
        .replace("/", "");
    let card_grid = actix_admin
        .card_grids
        .get(path.as_str())
        .ok_or_else(|| ActixAdminError::not_found("Card grid not found"))?;

    let entity_name = actix_admin
        .entity_names
        .values()
        .flatten()
        .find(|el| el.link == path)
        .map(|el| el.name.as_str())
        .unwrap_or("");

    let mut ctx = Context::new();
    ctx.insert("entity_name", entity_name);
    ctx.insert("entity_names", &actix_admin.entity_names);
    ctx.insert(
        "notifications",
        &Vec::<crate::ActixAdminNotification>::new(),
    );
    ctx.insert("card_grid", card_grid);

    add_auth_context(&session, actix_admin, &mut ctx).await;

    let body = actix_admin
        .tera
        .render("card_grid.html", &ctx)
        .map_err(|e| ActixAdminError::internal(format!("Template error: {e}")))?;
    Ok(html_response(StatusCode::OK, body))
}
