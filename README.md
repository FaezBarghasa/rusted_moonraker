# Rusted Moonraker (RMR)

Rusted Moonraker (RMR) is a high-performance, single-process API web server and touchscreen control panel for the [Klipper](https://github.com/Klipper3d/klipper) 3D printer firmware, ported from the Python Tornado-based Moonraker implementation to a unified Rust codebase. 

RMR incorporates Actix-web HTTP/WebSocket services, an asynchronous Unix Domain Socket (UDS) client actor communicating with Klippy, transactional database storage using SurrealDB, an optimized memory-mapped zero-copy G-code analyzer, and an integrated [Slint](https://slint.dev/) touchscreen GUI.

---

## Architectural Layout

The system is structured as a multi-crate Rust workspace:

- **[`rmr-core`](file:///home/jrad/RustroverProjects/rusted_moonraker/rmr-core)**: Core library housing configuration loaders, JSON-RPC communication drivers, database transactions, G-code analysis pipelines, Actix web servers, WebSocket routers, and authorization middleware.
- **[`rmr-gui`](file:///home/jrad/RustroverProjects/rusted_moonraker/rmr-gui)**: Embedded graphical touchscreen interface compiled with Slint. Includes high-frequency status readouts, temperature presets, macros, and emergency controls.
- **[`rmr-app`](file:///home/jrad/RustroverProjects/rusted_moonraker/rmr-app)**: Daemon launcher executable that starts background actors, SurrealDB storage engines, HTTP/WS endpoints, and loops the Slint GUI on the main thread.

---

## Installation & Building

### Prerequisites

You need a working Rust toolchain. Slint requires graphic backend libraries (like X11, Wayland, or KMS/DRM on Linux).

```bash
# Verify cargo is installed
cargo --version
```

### Build the Workspace

To compile Rusted Moonraker in debug mode:

```bash
cargo build
```

To compile a high-performance release binary:

```bash
cargo build --release
```

The resulting binary will be located at `target/release/rmr-app`.

---

## Running the Daemon

Launch the orchestrator by passing the path to the configuration file (or it will default to `~/.config/rmr/moonraker.conf`):

```bash
cargo run -p rmr-app -- [path/to/moonraker.conf]
```

---

## Running Tests

To execute the unit and integration test suites:

```bash
cargo test --workspace
```
