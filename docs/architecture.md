# Rusted Moonraker (RMR) Architecture

Rusted Moonraker (RMR) transitions the Moonraker backend from a dynamic, interpreted Python runtime (Tornado) to a statically typed, compiled, concurrent Rust system. To guarantee high performance, absolute thread safety, and zero UI stutter on embedded hardware, the system is built around several core components and design patterns.

---

## 1. Architectural Topology & Design Patterns

### The Actor Pattern for UDS Communication
The connection to Klipper via the Unix Domain Socket (UDS) is treated as an isolated, stateful Actor (`KlippyConnectionActor`).
- The actor owns the UDS socket connection and handles automatic recovery and exponential backoffs (ranging from 1s up to 32s limit).
- The actor communicates with the rest of the application exclusively through messages.
- Command requests are received via a multi-producer single-consumer (`mpsc`) channel.
- Telemetry updates (temperatures, position coordinates, printing status) are pushed out via a single-producer multi-consumer `watch` channel.

### SurrealDB Transactional Local Storage
Database management utilizes SurrealDB configured with the high-performance, transactional embedded `RocksDB` local storage engine (with `kv-mem` for testing).
- Defines structural schemas for `PrintRecord` and `WebLayoutPreset` using Serde serializations.
- Employs SurrealQL-based async CRUD functions for saving, retrieving, and deleting records.
- Ensures concurrent transaction safety across threads and prevents database corruption using transactional lock file checks.

### Memory-Mapped Zero-Copy G-code Analyzer
To prevent heavy files from blocking executor threads, the G-code parsing pipeline (`analyzer.rs`) maps uploaded G-code files directly into virtual memory using `memmap2::Mmap`.
- Executes an ultra-fast parallel backward search using the `rayon` crate, scanning chunks in parallel starting from the final byte.
- Parses out estimated print time (supporting both seconds and HMS formats), layer heights, slicer type, and raw base64 thumbnail bytes.
- Decodes and saves the extracted base64 preview image directly to `/opt/printer_data/thumbnails/` as a raw PNG, falling back to local user config paths if write permissions are restricted.

### Actix WebSocket Session Actors
WebSocket connections from Fluidd are managed using Actix Web Actors (`actix-web-actors`), implementing `Actor` and `StreamHandler` for `FluiddSession`.
- Session actors parse incoming JSON-RPC 2.0 messages from Fluidd and register subscribers.
- They stream system status updates safely by serializing state frames into text frames and broadcasting them asynchronously to all active client actors via a Tokio broadcast subscription.

The frontend runs as part of the same process, loading `MainWindow` from the `rmr-gui` crate.
- Pushes Klipper state updates from the watch channel to Slint properties on the main thread via thread-safe `slint::invoke_from_event_loop` handlers.
- Transmits user commands (Emergency Stop, G-code macros, temperature presets) back to the tokio runtime thread pool.

---

## 2. Workspace Crate Components

RMR is organized as a multi-crate workspace:

```mermaid
graph TD
    A[rmr-app] --> B[rmr-core]
    A --> C[rmr-gui]
    B --> D[SurrealDB]
    B --> E[Actix-web API]
    C --> F[Slint GUI]
```

### `rmr-core`
The engine of the application.
- `config/`: Parse INI config blocks and validate host ports.
- `db/`: SurrealDB instance wrapper, migration loader, and lockfiles.
- `klippy/`: JSON-RPC line codec and `KlippyConnectionActor`.
- `files/`: Memory-mapped metadata scanner and async directory crawler.
- `web/`: Actix routes, WebSocket frames broker, IP authorization filters.

### `rmr-gui`
Exposes the compiled GUI module interface and layout code (`ui/app.slint`).

### `rmr-app`
The executable entry point. Initializes the Tokio multi-threaded runtime, sets up DB and actors, spawns background web services, and drives the Slint window loop on the main thread.
