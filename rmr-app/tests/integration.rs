use actix_web::{test, web, App};
use actix_web::middleware::Compress;
use rmr_core::web::{handlers, AppState};
use rmr_core::db::init_db;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashSet;

#[actix_web::test]
async fn test_get_history() {
    let db = init_db("memory").await.unwrap();
    let ws_clients = Arc::new(RwLock::new(HashSet::new()));
    
    let app_state = web::Data::new(AppState {
        db,
        ws_clients,
    });

    let app = test::init_service(
        App::new()
            .app_data(app_state.clone())
            .wrap(Compress::default())
            .service(
                web::scope("/printer")
                    .route("/history", web::get().to(handlers::get_history))
            )
    ).await;

    let req = test::TestRequest::get().uri("/printer/history").to_request();
    let resp = test::call_service(&app, req).await;
    assert!(resp.status().is_success());
}