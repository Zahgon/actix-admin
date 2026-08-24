mod test_setup;
use test_setup::prelude::*;

#[cfg(test)]
mod post_delete_is_success {
    use super::{form_request_raw, request};
    use actix_admin::prelude::*;
    use itertools::Itertools;
    use sea_orm::{
        sea_query::{Expr, Value},
        ColumnTrait, EntityTrait, QueryFilter,
    };
    use tower::ServiceExt;

    use crate::create_app;

    #[tokio::test(flavor = "multi_thread")]
    async fn post_delete() {
        let db = super::setup_db(true).await;
        let app = create_app!(db, false, None, false);
        let id = 1;
        let entity = super::test_setup::Post::find_by_id(id)
            .one(&db)
            .await
            .unwrap();
        assert!(entity.is_some());

        let uri = format!("/admin/post/delete/{}", id);
        let req = request("DELETE", &uri);
        let resp = app.clone().oneshot(req).await.unwrap();

        // Delete should fail due to foreign key
        assert!(!resp.status().is_success());

        let comment_delete_res = super::test_setup::Comment::delete_by_id(id)
            .exec(&db)
            .await
            .unwrap();
        assert_eq!(comment_delete_res.rows_affected, 1);

        let uri = format!("/admin/post/delete/{}", id);
        let req = request("DELETE", &uri);
        let resp = app.clone().oneshot(req).await.unwrap();
        assert!(resp.status().is_success());

        let entity_after_delete = super::test_setup::Post::find_by_id(id)
            .one(&db)
            .await
            .unwrap();
        assert!(entity_after_delete.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn comment_delete() {
        let db = super::setup_db(true).await;
        let app = create_app!(db, false, None, false);
        let id = 1;
        let entity = super::test_setup::Comment::find_by_id(id)
            .one(&db)
            .await
            .unwrap();
        assert!(entity.is_some());

        let uri = format!("/admin/comment/delete/{}", id);
        let req = request("DELETE", &uri);
        let resp = app.clone().oneshot(req).await.unwrap();
        assert!(resp.status().is_success());

        let entity_after_delete = super::test_setup::Comment::find_by_id(id)
            .one(&db)
            .await
            .unwrap();
        assert!(entity_after_delete.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn comment_delete_many() {
        let db = super::setup_db(true).await;
        let app = create_app!(db, false, None, false);
        let ids = vec![1, 2, 3];
        for id in &ids {
            let entity = super::test_setup::Comment::find_by_id(*id)
                .one(&db)
                .await
                .unwrap();
            assert!(entity.is_some());
        }

        let payload: String = ids.iter().map(|i| format!("ids={}", i)).join("&");
        let ids_payload = payload;
        let req = form_request_raw("DELETE", "/admin/comment/delete", ids_payload);
        let resp = app.clone().oneshot(req).await.unwrap();
        assert!(resp.status().is_redirection());

        for id in ids {
            let entity_after_delete = super::test_setup::Comment::find_by_id(id)
                .one(&db)
                .await
                .unwrap();
            assert!(entity_after_delete.is_none());
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn post_delete_many() {
        let db = super::setup_db(true).await;
        let app = create_app!(db, false, None, false);
        let ids = vec![1, 2, 3];
        for id in &ids {
            let entity = super::test_setup::Post::find_by_id(*id)
                .one(&db)
                .await
                .unwrap();
            assert!(entity.is_some());
        }

        let payload: String = ids.iter().map(|i| format!("ids={}", i)).join("&");
        let ids_payload = payload;
        let req = form_request_raw("DELETE", "/admin/post/delete", ids_payload.clone());
        let resp = app.clone().oneshot(req).await.unwrap();

        // Fails because of FK constraints
        assert!(resp.status().is_server_error());

        // Remove FK
        let update_res = super::test_setup::Comment::update_many()
            .col_expr(
                super::test_setup::comment::Column::PostId,
                Expr::value(Value::Int(None)),
            )
            .filter(super::test_setup::comment::Column::PostId.is_in(ids.clone()))
            .exec(&db)
            .await;
        assert!(update_res.is_ok());

        // Delete again
        let req = form_request_raw("DELETE", "/admin/post/delete", ids_payload);
        let resp = app.clone().oneshot(req).await.unwrap();

        // Should not fail anymore and redirect correctly
        assert!(resp.status().is_redirection());

        for id in ids {
            let entity_after_delete = super::test_setup::Post::find_by_id(id)
                .one(&db)
                .await
                .unwrap();
            assert!(entity_after_delete.is_none());
        }
    }
}
