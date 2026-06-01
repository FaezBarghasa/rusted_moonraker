use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::{mpsc, broadcast};

use rmr_core::config::MoonrakerConfig;
use rmr_core::db::DatabaseManager;
use rmr_core::files::FileManager;
use rmr_core::klippy::{KlippyConnectionActor, KlippyCommand};
use rmr_core::web::{SystemState, start_telemetry_loop, start_web_server};
use rmr_gui::MainWindow;
use slint::ComponentHandle;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".config/rmr/moonraker.conf"))
                .unwrap_or_else(|| PathBuf::from("moonraker.conf"))
        });

    let config = if config_path.exists() {
        MoonrakerConfig::load_from_file(&config_path)?
    } else {
        println!("Config file not found at {:?}, using default settings.", config_path);
        MoonrakerConfig::load_from_str("")?
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let (_db, _file_manager, klippy_tx, state_rx, _ws_broadcast_tx, _system_state) = rt.block_on(async {
        let db_path = config.database.db_path.to_string_lossy().to_string();
        let db = DatabaseManager::initialize(std::path::Path::new(&db_path))
            .await
            .expect("Failed to initialize SurrealDB");

        let upload_dir = dirs::home_dir()
            .map(|h| h.join(".printer_data/gcodes"))
            .unwrap_or_else(|| PathBuf::from("./gcodes"));
        
        let file_manager = FileManager::new(
            upload_dir,
            db.clone(),
            config.server.max_upload_size_mb,
        );
        let _ = file_manager.sync_database().await;

        let (klippy_tx, klippy_rx) = mpsc::channel::<KlippyCommand>(100);
        let (actor, state_rx) = KlippyConnectionActor::spawn(
            config.klippy.uds_path.clone(),
            klippy_rx,
        );
        let (ws_broadcast_tx, _) = broadcast::channel(100);

        tokio::spawn(async move {
            let _ = actor.run_actor_loop().await;
        });

        let system_state = Arc::new(SystemState {
            config,
            db: db.clone(),
            file_manager,
            klippy_tx: klippy_tx.clone(),
            state_rx: state_rx.clone(),
            ws_broadcast_tx: ws_broadcast_tx.clone(),
            temp_history: std::sync::Mutex::new(Vec::new()),
        });

        start_telemetry_loop(system_state.clone());

        let state_clone = system_state.clone();
        tokio::spawn(async move {
            if let Err(e) = start_web_server(state_clone).await {
                eprintln!("Actix-web server encountered error: {:?}", e);
            }
        });

        (db, system_state.file_manager.clone(), klippy_tx, state_rx, ws_broadcast_tx, system_state)
    });

    let main_window = MainWindow::new()?;
    let handle = rt.handle().clone();

    let klippy_tx_clone = klippy_tx.clone();
    let handle_clone = handle.clone();
    main_window.on_emergency_stop(move || {
        let tx = klippy_tx_clone.clone();
        handle_clone.spawn(async move {
            let _ = tx.send(KlippyCommand::EmergencyStop).await;
        });
    });

    let klippy_tx_clone = klippy_tx.clone();
    let handle_clone = handle.clone();
    main_window.on_set_target_temperature(move |temp| {
        let tx = klippy_tx_clone.clone();
        handle_clone.spawn(async move {
            let params = serde_json::json!({
                "script": format!("M104 S{}", temp)
            });
            let (resp_tx, _) = tokio::sync::oneshot::channel();
            let _ = tx.send(KlippyCommand::JsonRpcRequest {
                method: "printer.gcode.script".to_string(),
                params,
                response_sender: resp_tx,
            }).await;
        });
    });

    let klippy_tx_clone = klippy_tx.clone();
    let handle_clone = handle.clone();
    main_window.on_send_gcode(move |gcode| {
        let tx = klippy_tx_clone.clone();
        let gcode_str = gcode.to_string();
        handle_clone.spawn(async move {
            let params = serde_json::json!({
                "script": gcode_str
            });
            let (resp_tx, _) = tokio::sync::oneshot::channel();
            let _ = tx.send(KlippyCommand::JsonRpcRequest {
                method: "printer.gcode.script".to_string(),
                params,
                response_sender: resp_tx,
            }).await;
        });
    });

    let mut state_rx_clone = state_rx.clone();
    let ui_weak = main_window.as_weak();
    rt.spawn(async move {
        loop {
            if state_rx_clone.changed().await.is_ok() {
                let current = state_rx_clone.borrow().clone();
                let ui = ui_weak.clone();
                
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui_instance) = ui.upgrade() {
                        ui_instance.set_print_status(slint::SharedString::from(&current.print_status));
                        ui_instance.set_tool_temp(current.tool_temp as f32);
                        ui_instance.set_target_temp(current.target_temp as f32);
                        ui_instance.set_print_progress(current.print_progress as f32);
                        ui_instance.set_pos_x(slint::SharedString::from(format!("{:.2}", current.x)));
                        ui_instance.set_pos_y(slint::SharedString::from(format!("{:.2}", current.y)));
                        ui_instance.set_pos_z(slint::SharedString::from(format!("{:.2}", current.z)));
                    }
                });
            }
        }
    });

    main_window.run()?;

    Ok(())
}
