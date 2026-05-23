# Weather Dashboard

A weather dashboard application built with React, TypeScript, and Chart.js.

## Tech Stack

- **Build Tool:** Vite 6
- **Framework:** React 18 with TypeScript
- **Charts:** Chart.js + react-chartjs-2
- **API:** Open-Meteo (free, no API key required)

## Getting Started

### Prerequisites

- Node.js 18+
- npm 9+

### Installation

```bash
npm install
```

### Development

```bash
npm run dev
```

The app will be available at [http://localhost:3000](http://localhost:3000).

### Build

```bash
npm run build
```

### Preview Production Build

```bash
npm run preview
```

## Project Structure

```
├── public/          # Static assets
├── src/             # Source code
│   ├── components/  # React components
│   ├── services/    # API services
│   ├── types/       # TypeScript types
│   ├── utils/       # Utility functions
│   ├── App.tsx      # Root component
│   └── main.tsx     # Entry point
├── index.html       # HTML template
├── package.json
├── tsconfig.json
└── vite.config.ts
```
