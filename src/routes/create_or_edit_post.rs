use super::helpers::{add_default_context_with_session, SearchParams};
use super::{render_create_or_edit_form, AdminAction, Params, RoutePrelude};
use crate::admin_prelude;
use crate::ActixAdminError;
use crate::ActixAdminNotification;
use crate::{prelude::*, ActixAdminErrorType};
use axum::extract::{FromRequest, Multipart, Path, RawQuery, Request};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use sea_orm::DatabaseConnection;
use std::collections::HashMap;
use std::sync::Arc;
use tera::Context;
use tower_sessions::Session;

use super::helpers::{html_response, run_local};

pub async fn create_post<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Extension(db): Extension<DatabaseConnection>,
    request: Request,
) -> Result<Response, ActixAdminError> {
    run_local(create_post_inner::<E>(
        session,
        headers,
        raw_query,
        actix_admin,
        db,
        request,
    ))
}

async fn create_post_inner<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    raw_query: Option<String>,
    actix_admin: Arc<ActixAdmin>,
    db: DatabaseConnection,
    request: Request,
) -> Result<Response, ActixAdminError> {
    let actix_admin = &actix_admin;
    let raw_query = raw_query.unwrap_or_default();
    // CSRF must be verified before the body is consumed: a Multipart extractor
    // argument would 400 a non-multipart body before the CSRF check, whereas
    // actix-web ran CSRF first and returned 403.
    verify_csrf(actix_admin, &session, &headers, &raw_query).await?;
    let payload = Multipart::from_request(request, &())
        .await
        .map_err(|e| ActixAdminError::bad_request(e.to_string()))?;
    let model = ActixAdminModel::create_from_payload(
        None,
        payload,
        &format!(
            "{}/{}",
            actix_admin.configuration.file_upload_directory,
            E::get_entity_name()
        ),
    )
    .await;
    create_or_edit_post::<E>(
        &session,
        &headers,
        &raw_query,
        &db,
        model,
        None::<E::Id>,
        actix_admin,
    )
    .await
}

pub async fn edit_post<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Extension(db): Extension<DatabaseConnection>,
    Path(id): Path<E::Id>,
    request: Request,
) -> Result<Response, ActixAdminError> {
    run_local(edit_post_inner::<E>(
        session,
        headers,
        raw_query,
        actix_admin,
        db,
        id,
        request,
    ))
}

async fn edit_post_inner<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    raw_query: Option<String>,
    actix_admin: Arc<ActixAdmin>,
    db: DatabaseConnection,
    id: E::Id,
    request: Request,
) -> Result<Response, ActixAdminError> {
    let actix_admin = &actix_admin;
    let raw_query = raw_query.unwrap_or_default();
    verify_csrf(actix_admin, &session, &headers, &raw_query).await?;
    let payload = Multipart::from_request(request, &())
        .await
        .map_err(|e| ActixAdminError::bad_request(e.to_string()))?;
    let model = ActixAdminModel::create_from_payload(
        Some(id.to_string()),
        payload,
        &format!(
            "{}/{}",
            actix_admin.configuration.file_upload_directory,
            E::get_entity_name()
        ),
    )
    .await;
    create_or_edit_post::<E>(
        &session,
        &headers,
        &raw_query,
        &db,
        model,
        Some(id),
        actix_admin,
    )
    .await
}

pub async fn create_or_edit_post<E: ActixAdminViewModelTrait>(
    session: &Session,
    headers: &HeaderMap,
    query: &str,
    db: &DatabaseConnection,
    model_res: Result<ActixAdminModel, ActixAdminError>,
    id: Option<E::Id>,
    actix_admin: &ActixAdmin,
) -> Result<Response, ActixAdminError> {
    let action = if id.is_some() {
        AdminAction::Edit
    } else {
        AdminAction::Create
    };
    // Note: multipart POSTs cannot re-verify CSRF from body here because
    // the payload was already consumed by `create_from_payload`; CSRF for
    // create/edit is asserted via the `_csrf` query param (see csrf.rs docs).
    let ctx = admin_prelude!(
        session,
        headers,
        query,
        actix_admin,
        RoutePrelude {
            action,
            verify_csrf: true,
            partial_unauth: true,
            with_auth_context: false,
        },
        E
    );

    let mut model = match model_res {
        Ok(m) => m,
        Err(e) => {
            // Fail closed on multipart/upload errors instead of panicking.
            return Err(ActixAdminError::bad_request(e.to_string()));
        }
    };
    let _ = E::validate_entity(&mut model, db).await;

    if model.has_errors() {
        let notif = vec![ActixAdminNotification::from(ActixAdminError {
            ty: ActixAdminErrorType::ValidationErrors,
            msg: String::new(),
        })];
        return render_create_or_edit_form::<E>(
            session,
            headers,
            query,
            actix_admin,
            ctx.view_model,
            db,
            ctx.entity_name,
            &model,
            ctx.tenant_ref,
            notif,
            ctx.view_model.inline_edit,
            StatusCode::OK,
        )
        .await;
    }

    let res = match id {
        Some(id) => E::edit_entity(db, id, model.clone(), ctx.tenant_ref).await,
        None => E::create_entity(db, model.clone(), ctx.tenant_ref).await,
    };

    match res {
        Ok(model) => {
            let params = Params::from_query(query);
            let search_params = SearchParams::from_params(&params, ctx.view_model);

            if ctx.view_model.inline_edit {
                let mut tctx = Context::new();
                tctx.insert("entity", &model);
                super::helpers::add_auth_context(session, actix_admin, &mut tctx).await;
                add_default_context_with_session(
                    &mut tctx,
                    headers,
                    ctx.view_model,
                    ctx.entity_name,
                    actix_admin,
                    Vec::new(),
                    &search_params,
                    Some(session),
                );
                let body = actix_admin
                    .tera
                    .render("list/row.html", &tctx)
                    .map_err(|e| ActixAdminError::internal(e.to_string()))?;
                Ok(html_response(StatusCode::OK, body))
            } else {
                Ok((
                    StatusCode::SEE_OTHER,
                    [(
                        header::LOCATION,
                        format!(
                            "{0}/{1}/list?{2}",
                            actix_admin.configuration.base_path,
                            ctx.entity_name,
                            search_params.to_query_string()
                        ),
                    )],
                )
                    .into_response())
            }
        }
        Err(e) => {
            render_create_or_edit_form::<E>(
                session,
                headers,
                query,
                actix_admin,
                ctx.view_model,
                db,
                ctx.entity_name,
                &model,
                ctx.tenant_ref,
                vec![ActixAdminNotification::from(e)],
                ctx.view_model.inline_edit,
                StatusCode::OK,
            )
            .await
        }
    }
}

#[doc(hidden)]
impl From<String> for ActixAdminModel {
    fn from(string: String) -> Self {
        // Parse application/x-www-form-urlencoded using the standard crate
        // rather than a bespoke hand-parser (which used to only decode `%3A`).
        let values: HashMap<String, String> =
            serde_urlencoded::from_str(&string).unwrap_or_default();

        ActixAdminModel {
            primary_key: None,
            values,
            errors: HashMap::new(),
            custom_errors: HashMap::new(),
            fk_values: HashMap::new(),
            display_name: None,
        }
    }
}
