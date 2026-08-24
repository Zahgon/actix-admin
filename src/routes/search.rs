use super::helpers::run_local;
use super::list::replace_regex;
use super::RoutePrelude;
use crate::admin_prelude;
use crate::prelude::*;
use axum::extract::RawQuery;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use sea_orm::DatabaseConnection;
use serde_derive::{Deserialize, Serialize};
use std::sync::Arc;
use tower_sessions::Session;

#[derive(Serialize)]
struct LabelValue {
    label: String,
    value: String,
}

#[derive(Serialize)]
struct SearchList {
    items: Vec<LabelValue>,
}

#[derive(Debug, Deserialize, Default)]
pub struct SearchParam {
    #[serde(default)]
    q: String,
}

pub async fn search<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Extension(db): Extension<DatabaseConnection>,
) -> Result<Response, ActixAdminError> {
    run_local(search_inner::<E>(
        session,
        headers,
        raw_query,
        actix_admin,
        db,
    ))
}

async fn search_inner<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    raw_query: Option<String>,
    actix_admin: Arc<ActixAdmin>,
    db: DatabaseConnection,
) -> Result<Response, ActixAdminError> {
    let db = &db;
    let actix_admin = &actix_admin;
    let raw_query = raw_query.unwrap_or_default();
    let ctx = admin_prelude!(
        &session,
        &headers,
        &raw_query,
        actix_admin,
        RoutePrelude::view(),
        E
    );

    let search_query: SearchParam = serde_urlencoded::from_str(&raw_query).unwrap_or_default();

    let params = ActixAdminViewModelParams {
        page: None,
        entities_per_page: None,
        viewmodel_filter: Vec::new(),
        search: search_query.q,
        sort_by: ctx.view_model.primary_key.clone(),
        sort_order: SortOrder::Asc,
        tenant_ref: ctx.tenant_ref,
    };

    // TODO: Improve by not loading all values (add a limit clause)
    let entities = match E::list(db, &params).await {
        Ok(res) => {
            let mut entities = res.1;
            replace_regex(ctx.view_model, &mut entities);
            entities
                .into_iter()
                .filter_map(|e| {
                    let value = e.primary_key?;
                    Some(LabelValue {
                        label: e.display_name.unwrap_or_default(),
                        value,
                    })
                })
                .collect()
        }
        Err(e) => return Err(ActixAdminError::internal(e.to_string())),
    };

    Ok(Json(SearchList { items: entities }).into_response())
}
