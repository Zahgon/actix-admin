use crate::prelude::*;
use axum::extract::RawQuery;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use csv::WriterBuilder;
use sea_orm::DatabaseConnection;
use serde_derive::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use tera::Context;
use tower_sessions::Session;

use super::helpers::{add_default_context_with_session, html_response, run_local, SearchParams};
use super::{add_auth_context, render_template, validate_sort_by, ListQuery, RoutePrelude};
use crate::admin_prelude;
use crate::ActixAdminModel;
use crate::ActixAdminNotification;
use crate::ActixAdminViewModel;
use crate::ActixAdminViewModelTrait;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl fmt::Display for SortOrder {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SortOrder::Asc => write!(f, "Asc"),
            SortOrder::Desc => write!(f, "Desc"),
        }
    }
}

pub fn replace_regex(view_model: &ActixAdminViewModel, models: &mut [ActixAdminModel]) {
    for field in view_model
        .fields
        .iter()
        .filter(|f| f.list_regex_mask.is_some())
    {
        let regex = field.list_regex_mask.as_ref().unwrap();
        for model in models.iter_mut() {
            if let Some(value) = model.values.get_mut(&field.field_name) {
                *value = regex.replace_all(value, "****").to_string();
            }
        }
    }
}

pub async fn export_csv<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Extension(db): Extension<DatabaseConnection>,
) -> Result<Response, ActixAdminError> {
    run_local(export_csv_inner::<E>(
        session,
        headers,
        raw_query,
        actix_admin,
        db,
    ))
}

async fn export_csv_inner<E: ActixAdminViewModelTrait>(
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
        RoutePrelude::export(),
        E
    );

    let query = ListQuery::from_query(&raw_query, ctx.view_model);
    validate_sort_by(ctx.view_model, &query.sort_by)?;

    let params = query.to_view_model_params(ctx.tenant_ref, false);

    let entities = match E::list(&db, &params).await {
        Ok(res) => {
            let mut entities = res.1;
            replace_regex(ctx.view_model, &mut entities);
            entities
        }
        Err(_) => Vec::new(),
    };

    let mut writer = WriterBuilder::new().from_writer(vec![]);
    let mut fields = ctx
        .view_model
        .fields
        .iter()
        .map(|f| f.field_name.clone())
        .collect::<Vec<_>>();
    fields.insert(0, ctx.view_model.primary_key.clone());
    writer
        .write_record(&fields)
        .map_err(|e| ActixAdminError::internal(e.to_string()))?;

    for entity in entities {
        let mut values = vec![entity.primary_key.unwrap_or_default()];
        for field in ctx.view_model.fields {
            let value = entity
                .values
                .get(&field.field_name)
                .cloned()
                .unwrap_or_default();
            values.push(
                entity
                    .fk_values
                    .get(&field.field_name)
                    .cloned()
                    .unwrap_or(value),
            );
        }
        writer
            .write_record(&values)
            .map_err(|e| ActixAdminError::internal(e.to_string()))?;
    }

    let body = writer
        .into_inner()
        .map_err(|e| ActixAdminError::internal(e.to_string()))?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/csv"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"export.csv\"",
            ),
        ],
        body,
    )
        .into_response())
}

pub async fn list<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Extension(db): Extension<DatabaseConnection>,
) -> Result<Response, ActixAdminError> {
    run_local(list_inner::<E>(session, headers, raw_query, actix_admin, db))
}

async fn list_inner<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    raw_query: Option<String>,
    actix_admin: Arc<ActixAdmin>,
    db: DatabaseConnection,
) -> Result<Response, ActixAdminError> {
    let actix_admin = &actix_admin;
    let raw_query = raw_query.unwrap_or_default();
    let route_ctx = admin_prelude!(
        &session,
        &headers,
        &raw_query,
        actix_admin,
        RoutePrelude::view(),
        E
    );

    let query = ListQuery::from_query(&raw_query, route_ctx.view_model);
    validate_sort_by(route_ctx.view_model, &query.sort_by)?;

    let mut ctx = Context::new();
    add_auth_context(&session, actix_admin, &mut ctx).await;

    let vm_params = query.to_view_model_params(route_ctx.tenant_ref, true);
    let search_params = SearchParams::from_list_query(&query);

    let (num_pages, mut entities) = match E::list(&db, &vm_params).await {
        Ok(res) => res,
        Err(e) => {
            ctx.insert("entities", &Vec::<ActixAdminModel>::new());
            ctx.insert("num_pages", &0);
            ctx.insert("min_show_page", &1);
            ctx.insert("max_show_page", &1);
            ctx.insert("page", &1);
            ctx.insert("notifications", &[ActixAdminNotification::from(e)]);
            return Ok(html_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                render_template(&actix_admin.tera, "list.html", &ctx)
                    .map_err(|e| ActixAdminError::internal(e.to_string()))?,
            ));
        }
    };

    // Best-effort in-memory sort on the FK display column when the user
    // requested to sort by a foreign-key field. This only re-orders within
    // the current page (the primary query still orders on the FK id) but
    // gives visually correct alphabetical order in the common case of
    // browsing without paging past `entities_per_page`.
    if let Some(field) = route_ctx
        .view_model
        .fields
        .iter()
        .find(|f| f.field_name == query.sort_by)
    {
        if !field.foreign_key.is_empty() {
            let asc = matches!(query.sort_order, SortOrder::Asc);
            entities.sort_by(|a, b| {
                let av = a
                    .fk_values
                    .get(&field.field_name)
                    .cloned()
                    .unwrap_or_default();
                let bv = b
                    .fk_values
                    .get(&field.field_name)
                    .cloned()
                    .unwrap_or_default();
                if asc {
                    av.cmp(&bv)
                } else {
                    bv.cmp(&av)
                }
            });
        }
    }

    replace_regex(route_ctx.view_model, &mut entities);
    let num_pages = num_pages.unwrap_or(1);
    let page = query.page.min(num_pages);
    let min_show_page = page.saturating_sub(4).max(1);
    let max_show_page = (page + 4).min(num_pages);

    add_default_context_with_session(
        &mut ctx,
        &headers,
        route_ctx.view_model,
        route_ctx.entity_name,
        actix_admin,
        Vec::new(),
        &search_params,
        Some(&session),
    );

    ctx.insert("entities", &entities);
    ctx.insert("num_pages", &num_pages);
    ctx.insert("min_show_page", &min_show_page);
    ctx.insert("max_show_page", &max_show_page);
    let mut viewmodel_filter = E::get_viewmodel_filter(&db).await;
    // Round-trip the current query's filter values back into the view-model
    // filter map so templates can pre-select them (and so re-submitting the
    // filter form doesn't silently clear the current selection).
    for f in &query.filters {
        if let Some(entry) = viewmodel_filter.get_mut(&f.name) {
            entry.value = f.value.clone();
            entry.operator = f.operator.clone();
        }
    }
    ctx.insert("viewmodel_filter", &viewmodel_filter);

    Ok(html_response(
        StatusCode::OK,
        render_template(&actix_admin.tera, "list.html", &ctx)
            .map_err(|err| ActixAdminError::internal(format!("{err:?}")))?,
    ))
}
