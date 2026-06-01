# Welcome to Rusted Moonraker (RMR) Documentation

Rusted Moonraker (RMR) is a high-performance Rust-based API web server and touchscreen control interface that exposes APIs for interacting with the [Klipper](https://github.com/Klipper3d/klipper) 3D printing firmware. 

RMR replaces the legacy Python Tornado backend with a statically typed, concurrent system built around Actix-web, SurrealDB, and Slint.

## Documentation Sections

- **[Architecture Details](architecture.md)**: Explore RMR's actor-based UDS communication topology, thread synchronization, and zero-copy G-code metadata scanner.
- **[Configuration Guide](configuration.md)**: Details on settings for `[server]`, `[klippy]`, and `[database]` config blocks.
- **[Installation Guide](installation.md)**: Step-by-step setup guides for compiling and running the daemon on Linux systems and embedded hardware.
- **[API Reference](external_api/introduction.md)**: Comprehensive guide on the HTTP/WebSocket endpoints and authorization filters.
