# Alarm Project Frontend

A modern React application for managing alarms with a beautiful UI built using shadcn-ui and Tailwind CSS.

## Prerequisites

- [Bun](https://bun.sh) 1.x

## Setup

1. Install dependencies:
```bash
bun install
```

2. Start the development server:
```bash
bun run dev
```

The application will be available at `http://localhost:5173`

## Building for Production

1. Build the application:
```bash
bun run build
```

2. The built files will be in the `dist` directory

## Development

### Project Structure

- `src/`
  - `components/`: Reusable UI components
  - `hooks/`: Custom React hooks
  - `lib/`: Utility functions and configurations
  - `pages/`: Page components
  - `services/`: Api clients
  - `types/`: TypeScript type definitions
  - `App.tsx`: Main application component
  - `main.tsx`: Application entry point
  - `config.ts`: Application config
  - `consts.ts`: Application consts
