#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KlipperStateTree {
    pub tool_temp: f64,
    pub target_temp: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub e: f64,
    pub print_status: String, // "Idle", "Printing", "Paused"
    pub print_progress: f64,
}

impl Default for KlipperStateTree {
    fn default() -> Self {
        KlipperStateTree {
            tool_temp: 0.0,
            target_temp: 0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            e: 0.0,
            print_status: "Idle".to_string(),
            print_progress: 0.0,
        }
    }
}

#[derive(Clone)]
pub struct StateStore {
    receiver: tokio::sync::watch::Receiver<KlipperStateTree>,
}

impl StateStore {
    pub fn new(receiver: tokio::sync::watch::Receiver<KlipperStateTree>) -> Self {
        StateStore { receiver }
    }

    pub fn get(&self) -> KlipperStateTree {
        self.receiver.borrow().clone()
    }

    pub fn get_receiver(&self) -> tokio::sync::watch::Receiver<KlipperStateTree> {
        self.receiver.clone()
    }
}
