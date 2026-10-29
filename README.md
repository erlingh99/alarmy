# Alarmy

A sunrise alarm system with a web interface and Raspberry Pi Pico W based light control. Wake up naturally with a simulated sunrise.

## Project Overview

This project consists of three main components:

- **Frontend**: React-based web interface for managing alarms
- **Backend**: FastAPI server for alarm storage and management 
- **Embedded**: Raspberry Pi Pico W controller for LED and buzzer control in 100% heapless Rust

## Quick Start

### Backend & Frontend

```bash
# Start the complete system using Docker
docker compose up -d --build
```

The web interface will be available at localhost port 80

### Pico W Setup

1. Flash the firmware following instructions in `/embedded/README.md`
2. Wire up a breadboard or use the PCB
3. Power up the device

## Development

See the individual component READMEs for detailed development instructions:

- [Frontend Documentation](frontend/README.md)
- [Backend Documentation](backend/README.md)
- [Embedded Documentation](embedded/README.md)
