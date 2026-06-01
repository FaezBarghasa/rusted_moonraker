use tokio::sync::{mpsc, broadcast};
use rmr_core::config::MoonrakerConfig;
use rmr_core::db::DatabaseManager;
use rmr_core::files::FileManager;
use rmr_core::klippy::{KlippyConnectionActor, KlippyCommand};
use rmr_core::web::SystemState;

#[tokio::test]
async fn test_orchestration_setup() {
    let temp_dir = std::env::temp_dir();
    let config_str = format!(
        "[server]\nhost: 127.0.0.1\nport: 8085\n[database]\ndb_path: {}\n[klippy]\nuds_path: /tmp/klippy.sock",
        temp_dir.join("rmr_integ_db").to_string_lossy()
    );
    
    let config = MoonrakerConfig::load_from_str(&config_str).unwrap();
    
    let db = DatabaseManager::initialize_mem().await.unwrap();
    let file_manager = FileManager::new(
        temp_dir.join("rmr_integ_gcodes"),
        db.clone(),
        config.server.max_upload_size_mb,
    );

    let (klippy_tx, _klippy_rx) = mpsc::channel::<KlippyCommand>(10);
    let (_actor, state_rx) = KlippyConnectionActor::spawn(
        config.klippy.uds_path.clone(),
        _klippy_rx,
    );
    let (ws_broadcast_tx, _) = broadcast::channel(10);

    let system_state = std::sync::Arc::new(SystemState {
        config,
        db,
        file_manager,
        klippy_tx,
        state_rx,
        ws_broadcast_tx,
        temp_history: std::sync::Mutex::new(Vec::new()),
    });

    assert_eq!(system_state.config.server.host, "127.0.0.1");
    assert_eq!(system_state.config.server.port, 8085);
}
