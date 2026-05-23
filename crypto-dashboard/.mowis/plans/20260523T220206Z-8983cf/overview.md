<plan>
[[task]]
id = "t1"
title = "Project scaffolding and dependency installation"
description = "Create a Vite + React + TypeScript project at the root. Run: npm create vite@latest . -- --template react-ts (or equivalent). Install dependencies: chart.js, react-chartjs-2, @types/chart.js. Create the following files:

- package.json (with name 'crypto-dashboard', scripts: dev, build, preview)
- vite.config.ts (standard React config)
- tsconfig.json (standard strict TS config)
- tsconfig.node.json
- index.html (minimal, with div#root, no custom fonts, title 'Crypto Dashboard')
- .gitignore (node_modules, dist, .env)
- README.md with project description, setup instructions (npm install, npm run dev), and note that it uses CoinGecko free API with no keys required

Do NOT create src/ files yet — those are handled by later tasks."
deps = []
model_tier = "fast"
tool_budget = 20
files_hint = ["package.json", "vite.config.ts", "tsconfig.json", "tsconfig.node.json", "index.html", ".gitignore", "README.md"]
</plan>

[[task]]
id = "t2"
title = "TypeScript types, utility functions, and global CSS"
description = "Create these files in the project:

1. src/types.ts — Define all TypeScript interfaces/types:
   - Coin: { id: string, symbol: string, name: string, image: string, current_price: number, market_cap: number, market_cap_rank: number, total_volume: number, price_change_percentage_24h: number, price_change_percentage_7d_in_currency?: number, sparkline_in_7d?: { price: number[] }, circulating_supply: number, ath: number, ath_change_percentage: number }
   - CoinChartData: { prices: [number, number][], market_caps: [number, number][], total_volumes: [number, number][] }
   - PortfolioHolding: { id: string, coinId: string, coinName: string, coinSymbol: string, amount: number, buyPrice: number, addedAt: string }
   - ChartTimeframe: '1' | '7' | '30' | '90' | '365'

2. src/utils/format.ts — Utility functions:
   - formatCurrency(value: number): string — formats as USD with commas, 2 decimal places for values < 1, more precision for very small values
   - formatCompact(value: number): string — formats large numbers as 1.2B, 345M, etc.
   - formatPercent(value: number): string — formats with + prefix and % suffix, 2 decimal places
   - formatSupply(value: number): string — formats with appropriate suffix

3. src/index.css — Global styles. CRITICAL design rules:
   - CSS reset (box-sizing, margin, padding)
   - body: background #FFFFFF, color #171717, font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif
   - NO gradients anywhere
   - NO emoji
   - NO purple colors
   - Blue accent: #2563EB for links, buttons, active states
   - Border color: #E5E7EB
   - Secondary text: #6B7280
   - Subtle card style: background #FFFFFF, border: 1px solid #E5E7EB, border-radius: 6px
   - Table styles: clean borders, hover state with #F9FAFB background
   - Button styles: primary (#2563EB bg, white text), secondary (white bg, #374151 text, #E5E7EB border)
   - Input styles: border 1px solid #E5E7EB, border-radius 6px, padding 8px 12px
   - Responsive: mobile-first, max-width container at 1200px
   - Scrollbar styling: thin, subtle
   - Transitions: all interactive elements have transition: all 0.15s ease
   - Badge style for positive (background #ECFDF5, color #059669) and negative (background #FEF2F2, color #DC2626) changes"
deps = ["t1"]
model_tier = "fast"
tool_budget = 25
files_hint = ["src/types.ts", "src/utils/format.ts", "src/index.css"]
</plan>

[[task]]
id = "t3"
title = "CoinGecko API service layer"
description = "Create src/api/coingecko.ts — a clean API service module.

Base URL: https://api.coingecko.com/api/v3

Functions to implement (all return typed promises, all handle errors gracefully with try/catch):

1. fetchTopCoins(perPage = 20, page = 1): Promise<Coin[]>
   - GET /coins/markets?vs_currency=usd&order=market_cap_desc&per_page={perPage}&page={page}&sparkline_in_7d=true&price_change_percentage=7d
   - Returns the array directly from response

2. fetchCoinChart(coinId: string, days: number): Promise<CoinChartData>
   - GET /coins/{coinId}/market_chart?vs_currency=usd&days={days}
   - Returns { prices, market_caps, total_volumes }

3. fetchCoinPrice(coinIds: string[]): Promise<Record<string, { usd: number, usd_24h_change: number }>>
   - GET /simple/price?ids={joined}&vs_currency=usd&include_24hr_change=true
   - Used for quick portfolio price updates

Each function should:
- Use fetch() with proper headers (Accept: application/json)
- Throw descriptive errors on non-2xx responses
- Include a comment noting the free tier rate limit (10-30 req/min)

Also export a constant COINS_PER_PAGE = 20."
deps = ["t1"]
model_tier = "fast"
tool_budget = 20
files_hint = ["src/api/coingecko.ts"]
</plan>

[[task]]
id = "t4"
title = "Custom React hooks for data fetching and portfolio"
description = "Create two custom hook files:

1. src/hooks/useCryptoData.ts
   - useCryptoData() hook that:
     - Fetches top coins on mount using fetchTopCoins()
     - Auto-refreshes every 60 seconds
     - Returns { coins, loading, error, lastUpdated, refetch }
     - Uses useState and useEffect with setInterval
     - Handles loading and error states
     - lastUpdated is a Date object

   - useCoinChart(coinId: string | null, days: number) hook that:
     - Fetches chart data when coinId or days changes using fetchCoinChart()
     - Returns { data, loading, error }
     - Returns null data if coinId is null

2. src/hooks/usePortfolio.ts
   - Manages portfolio holdings in localStorage (key: 'crypto-portfolio')
   - Returns { holdings, addHolding, removeHolding, updateHolding, totalValue, totalCost, totalPnL, holdingsWithCurrentPrice }
   - addHolding takes { coinId, coinName, coinSymbol, amount, buyPrice }
   - Uses fetchCoinPrice() periodically (every 60s) to get current prices for held coins
   - totalPnL = sum of (currentPrice - buyPrice) * amount for each holding
   - holdingsWithCurrentPrice merges holdings with live price data
   - Generate unique IDs using crypto.randomUUID() or Date.now() + Math.random()

Import types from '../types' and API functions from '../api/coingecko'.
Import formatCurrency from '../utils/format' if needed."
deps = ["t2", "t3"]
model_tier = "fast"
tool_budget = 25
files_hint = ["src/hooks/useCryptoData.ts", "src/hooks/usePortfolio.ts"]
</plan>

[[task]]
id = "t5"
title = "Market view components — PriceTable, Sparkline, SearchFilter"
description = "Create these component files (all in src/components/):

1. Sparkline.tsx
   - Props: { data: number[], color?: string, width?: number, height?: number }
   - Renders a tiny inline SVG sparkline chart (NO chart.js — just raw SVG)
   - Use SVG polyline with the data points mapped to x/y coordinates
   - Default: width=120, height=32, color=#2563EB
   - Green (#059669) if last > first, red (#DC2626) if last < first
   - Clean, no axes, just the line

2. SearchFilter.tsx
   - Props: { value: string, onChange: (value: string) => void }
   - Clean search input with a simple search icon (SVG, no emoji)
   - Placeholder: 'Search coins...'
   - Styled consistently with global CSS

3. PriceTable.tsx
   - Props: { coins: Coin[], onSelectCoin: (coinId: string) => void, selectedCoinId: string | null, loading: boolean }
   - Renders a responsive table with columns: #, Name (with icon + name + symbol), Price, 24h Change, Market Cap, Volume, 7d Chart
   - Coin image: 24px circle
   - Price formatted with formatCurrency
   - 24h change: green/red badge using formatPercent
   - Market cap and volume: formatCompact
   - 7d chart: inline Sparkline component using sparkline_in_7d.price data
   - Clickable rows — highlight selected row with subtle blue-tinted background (#EFF6FF)
   - Loading state: show 10 skeleton rows with pulsing animation
   - Include SearchFilter above the table and filter coins by name/symbol
   - Responsive: on mobile, hide Market Cap and Volume columns, show only Rank, Name, Price, Change

All components should be clean, minimal, no decorative elements. Use proper semantic HTML (table, thead, tbody, tr, th, td)."
deps = ["t2"]
model_tier = "fast"
tool_budget = 30
files_hint = ["src/components/Sparkline.tsx", "src/components/SearchFilter.tsx", "src/components/PriceTable.tsx"]
</plan>

[[task]]
id = "t6"
title = "Coin detail and chart component"
description = "Create src/components/CoinDetail.tsx

This component shows a detailed view of a selected cryptocurrency with a price chart.

Props: { coin: Coin | null, chartData: CoinChartData | null, chartLoading: boolean, timeframe: ChartTimeframe, onTimeframeChange: (tf: ChartTimeframe) => void }

Layout:
- Top section: coin image (40px), name, symbol, current price (large, 28px), 24h change badge
- Stats grid below price: Market Cap, 24h Volume, Circulating Supply, ATH, ATH Change — displayed in a clean grid of stat cards
- Chart section: Use chart.js (react-chartjs-2) Line chart
  - Register: CategoryScale, LinearScale, PointElement, LineElement, Tooltip, Filler
  - Line color: #2563EB
  - Fill: subtle gradient from #2563EB to transparent (use canvas gradient, NOT CSS gradient) — or just no fill, clean line only
  - Actually NO fill — just a clean line, 2px width
  - X axis: time labels formatted as dates
  - Y axis: price formatted with formatCurrency
  - Grid lines: very subtle #F3F4F6
  - Tooltip: dark background (#1F2937), white text, show price + date
  - Responsive, maintain aspect ratio
- Timeframe selector: buttons for 1D, 7D, 1M, 3M, 1Y — clean button group, active button has #2563EB background
- If no coin selected, show an empty state: 'Select a cryptocurrency to view details' with subtle text

Chart.js configuration should be production quality — proper formatting, no flickering, smooth animations (transition: 300ms).

Import Chart components from 'react-chartjs-2' and register from 'chart.js'."
deps = ["t2"]
model_tier = "fast"
tool_budget = 30
files_hint = ["src/components/CoinDetail.tsx"]
</plan>

[[task]]
id = "t7"
title = "Portfolio components — Portfolio tracker and Add holding form"
description = "Create these component files (all in src/components/):

1. Portfolio.tsx
   - Props: { holdings: PortfolioHolding[], totalValue: number, totalCost: number, totalPnL: number, onRemove: (id: string) => void, onAdd: () => void }
   - Summary section at top: 3 stat cards in a row
     - Total Value (large number, formatCurrency)
     - Total P&L (green if positive, red if negative, with formatPercent of P&L percentage)
     - Holdings count
   - Holdings table: Coin, Amount, Buy Price, Current Value, P&L, Actions
   - P&L column: colored badge (green/red) showing dollar amount and percentage
   - Actions column: remove button (subtle, text-only or simple X icon)
   - Empty state: 'No holdings yet' with a button to add first holding
   - 'Add Holding' button in the header area

2. AddHoldingModal.tsx
   - Props: { isOpen: boolean, onClose: () => void, onAdd: (holding: { coinId: string, coinName: string, coinSymbol: string, amount: number, buyPrice: number }) => void, availableCoins: Coin[] }
   - Modal overlay with centered content
   - Form fields:
     - Coin selector: searchable dropdown/filter from availableCoins (show name + symbol)
     - Amount: number input
     - Buy Price: number input with USD prefix
   - Form validation: all fields required, amount > 0, buyPrice > 0
   - Submit and Cancel buttons
   - Close on overlay click and Escape key
   - Clean modal style: white background, border, no heavy shadow, border-radius 8px
   - Overlay: rgba(0,0,0,0.3) background

Both components should use proper form elements, labels, and accessibility attributes."
deps = ["t2"]
model_tier = "fast"
tool_budget = 30
files_hint = ["src/components/Portfolio.tsx", "src/components/AddHoldingModal.tsx"]
</plan>

[[task]]
id = "t8"
title = "App assembly — main App component and entry point"
description = "Create these files to wire everything together:

1. src/App.tsx — The main application component
   - Layout: vertical layout with Header at top, then a responsive two-column layout
   - Header: app title 'Crypto Dashboard' (left), last updated time (right, subtle text), refresh button
   - Left column (60% on desktop, 100% on mobile): PriceTable + CoinDetail stacked vertically
   - Right column (40% on desktop, 100% on mobile): Portfolio
   - State management:
     - selectedCoinId: string | null (default null)
     - chartTimeframe: ChartTimeframe (default '7')
     - showAddModal: boolean
   - Use useCryptoData() for market data
   - Use useCoinChart(selectedCoinId, Number(chartTimeframe)) for chart
   - Use usePortfolio() for portfolio
   - Wire onSelectCoin to set selectedCoinId
   - Wire portfolio add/remove
   - The AddHoldingModal receives coins from useCryptoData as availableCoins
   - When no coin is selected, CoinDetail shows empty state
   - Tab/section switcher for mobile: toggle between Market and Portfolio views

   CSS for App layout should be in App.module.css or inline in the component using clean layout classes:
   - CSS Grid or Flexbox for the two-column layout
   - Gap: 24px
   - Max-width: 1200px, centered
   - Padding: 24px on desktop, 16px on mobile

2. src/main.tsx — Entry point
   - Import React, ReactDOM
   - Import './index.css'
   - Import App
   - Render App in StrictMode into #root

Make sure everything compiles and works end-to-end. The app should be a fully functional crypto dashboard."
deps = ["t4", "t5", "t6", "t7"]
model_tier = "fast"
tool_budget = 25
files_hint = ["src/App.tsx", "src/main.tsx"]
</plan>
</plan>