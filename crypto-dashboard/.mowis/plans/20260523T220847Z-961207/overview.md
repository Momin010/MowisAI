<plan>
[[task]]
id = "t1"
title = "Create formatting utilities"
description = "Create src/utils/format.ts with these exported functions using ES module exports:

1. `formatCurrency(value: number): string` — Format as USD with 2 decimals for values >= 1, up to 6 decimals for small values. Use Intl.NumberFormat with 'en-US' locale, USD currency. Example: 1234.56 → '$1,234.56', 0.001234 → '$0.001234'
2. `formatPercent(value: number): string` — Format with + or - prefix, 2 decimals, % suffix. Example: 2.34 → '+2.34%', -1.23 → '-1.23%'
3. `formatNumber(value: number): string` — Format with commas, no currency symbol. Example: 1234567 → '1,234,567'
4. `formatCompact(value: number): string` — Abbreviate large numbers with B/M/K suffixes. Example: 1234567890 → '$1.23B', 456789012 → '$456.79M', 12345 → '$12.35K'. Below 1000, just formatCurrency.
5. `formatPrice(value: number): string` — Smart price formatting: use 2 decimals for values >= 1, 4 decimals for >= 0.01, 6 decimals for >= 0.0001, 8 decimals otherwise. Always prefixed with $.

All functions must handle NaN, undefined, null gracefully by returning '$0.00', '0%', '0', '$0', '$0.00' respectively. Pure TypeScript, no external dependencies."
deps = []
model_tier = "fast"
tool_budget = 20
files_hint = ["src/utils/format.ts"]

[[task]]
id = "t2"
title = "Create Sparkline SVG component"
description = "Create src/components/Sparkline.tsx — a pure inline SVG sparkline chart component.

Props interface:
```
interface SparklineProps {
  data: number[];
  width?: number;     // default 120
  height?: number;    // default 40
  color?: string;     // default '#2563EB'
  positive?: boolean; // if undefined, auto-detect from first vs last value
}
```

Implementation:
- Use a <svg> element with viewBox matching width/height
- Calculate min/max from data to scale points to fill the SVG area
- Create a <polyline> for the line path with strokeWidth=1.5, fill=none
- Color: use #10B981 for positive (green), #EF4444 for negative (red), or the provided color prop
- Optional: add a subtle <linearGradient> fill area below the line (very low opacity, same color)
- The SVG should have no border, no background — just the line
- Smooth the path slightly using SVG line segments (no need for bezier, just connect the dots)
- Handle edge cases: empty data array, single data point

TypeScript React component. No external chart libraries — pure SVG."
deps = []
model_tier = "fast"
tool_budget = 20
files_hint = ["src/components/Sparkline.tsx"]

[[task]]
id = "t3"
title = "Create AddHoldingModal component"
description = "Create src/components/AddHoldingModal.tsx — a modal dialog for adding a crypto holding to the portfolio.

Props interface:
```
interface AddHoldingModalProps {
  coins: CoinMarket[];  // from src/types/coingecko.ts (has id, name, symbol, image, current_price)
  onAdd: (holding: { coinId: string; coinName: string; coinSymbol: string; amount: number; buyPrice: number }) => void;
  onClose: () => void;
}
```

PortfolioHolding type (define locally or import — this is the shape onAdd receives without id/addedAt):
```
{ coinId: string; coinName: string; coinSymbol: string; amount: number; buyPrice: number }
```

Implementation:
- Full-screen overlay with semi-transparent backdrop (#000000 at 50% opacity)
- Centered modal card: white bg, 1px solid #E5E7EB border, max-width 480px, padding 24px
- Header: 'Add Holding' title, X close button (top right)
- Search input to filter coins list — filters by name or symbol, case-insensitive
- Scrollable coin list below search (max-height 200px, overflow-y auto) — each item shows coin image (24x24), name, symbol, current price
- Clicking a coin selects it (highlight with #2563EB left border)
- Two number inputs below: 'Amount' and 'Buy Price (USD)' — both type=number, step=any
- Buy Price should pre-fill with the selected coin's current_price
- 'Add to Portfolio' button (bg #2563EB, white text, full width, disabled until coin selected and amount > 0)
- Close on backdrop click and Escape key
- All inputs: 1px solid #D1D5DB border, 8px padding, font-size 14px, focus outline #2563EB

Use React.FC with TypeScript. CSS classes defined in the component or reference class names that match src/index.css."
deps = []
model_tier = "fast"
tool_budget = 25
files_hint = ["src/components/AddHoldingModal.tsx"]

[[task]]
id = "t4"
title = "Create base CSS styles"
description = "REPLACE src/index.css with a complete, production-quality stylesheet. This is the ONLY CSS file — no CSS modules, no styled-components.

Design tokens:
- --color-bg: #FFFFFF
- --color-surface: #F9FAFB
- --color-border: #E5E7EB
- --color-text: #111827
- --color-text-secondary: #6B7280
- --color-accent: #2563EB
- --color-accent-hover: #1D4ED8
- --color-green: #10B981
- --color-red: #EF4444
- --font-stack: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif

Global styles:
- *, *::before, *::after: box-sizing border-box, margin 0, padding 0
- body: font-family var(--font-stack), background var(--color-bg), color var(--color-text), line-height 1.5, -webkit-font-smoothing antialiased
- System font stack only. NO @font-face, NO Google Fonts.

Layout classes:
- `.app` — max-width 1200px, margin 0 auto, padding 0 24px
- `.header` — display flex, align-items center, justify-content space-between, padding 20px 0, border-bottom 1px solid var(--color-border), margin-bottom 32px
- `.header h1` — font-size 20px, font-weight 600, letter-spacing -0.02em
- `.nav-tabs` — display flex, gap 4px, background var(--color-surface), padding 4px, border-radius 8px
- `.nav-tab` — padding 8px 16px, font-size 14px, font-weight 500, border-radius 6px, border none, background transparent, cursor pointer, color var(--color-text-secondary), transition all 0.15s
- `.nav-tab.active` — background white, color var(--color-text), box-shadow 0 1px 3px rgba(0,0,0,0.1)
- `.nav-tab:hover:not(.active)` — color var(--color-text)

Card/component classes:
- `.card` — background white, border 1px solid var(--color-border), border-radius 8px
- `.search-input` — width 100%, padding 10px 12px, border 1px solid var(--color-border), border-radius 6px, font-size 14px, outline none, transition border-color 0.15s
- `.search-input:focus` — border-color var(--color-accent), box-shadow 0 0 0 3px rgba(37,99,235,0.1)
- `.btn` — padding 8px 16px, border-radius 6px, font-size 14px, font-weight 500, cursor pointer, border 1px solid var(--color-border), background white, transition all 0.15s
- `.btn-primary` — background var(--color-accent), color white, border-color var(--color-accent)
- `.btn-primary:hover` — background var(--color-accent-hover)
- `.btn-primary:disabled` — opacity 0.5, cursor not-allowed

Table classes:
- `.price-table` — width 100%, border-collapse collapse
- `.price-table th` — text-align left, font-size 12px, font-weight 500, color var(--color-text-secondary), text-transform uppercase, letter-spacing 0.05em, padding 12px 16px, border-bottom 1px solid var(--color-border)
- `.price-table td` — padding 12px 16px, font-size 14px, border-bottom 1px solid var(--color-border)
- `.price-table tr` — cursor pointer, transition background 0.1s
- `.price-table tr:hover` — background var(--color-surface)
- `.price-table .coin-info` — display flex, align-items center, gap 12px
- `.price-table .coin-image` — width 32px, height 32px, border-radius 50%
- `.price-table .coin-name` — font-weight 500
- `.price-table .coin-symbol` — color var(--color-text-secondary), text-transform uppercase, font-size 12px, margin-left 4px
- `.price-table .positive` — color var(--color-green)
- `.price-table .negative` — color var(--color-red)
- `.price-table .rank` — color var(--color-text-secondary), font-size 13px

Portfolio classes:
- `.portfolio-header` — display flex, justify-content space-between, align-items center, margin-bottom 24px
- `.portfolio-total` — font-size 32px, font-weight 600, letter-spacing -0.02em
- `.portfolio-change` — font-size 14px, margin-top 4px
- `.holdings-list` — display flex, flex-direction column, gap 8px
- `.holding-item` — display flex, align-items center, justify-content space-between, padding 16px, border 1px solid var(--color-border), border-radius 8px
- `.holding-info` — display flex, align-items center, gap 12px
- `.holding-values` — text-align right
- `.holding-pnl` — font-size 13px, margin-top 2px

Detail/Chart classes:
- `.detail-header` — display flex, align-items center, gap 12px, margin-bottom 24px
- `.back-btn` — background none, border 1px solid var(--color-border), border-radius 6px, padding 6px 12px, cursor pointer, font-size 14px, color var(--color-text-secondary)
- `.detail-title` — font-size 24px, font-weight 600
- `.detail-price` — font-size 36px, font-weight 700, letter-spacing -0.02em, margin 8px 0
- `.detail-stats` — display grid, grid-template-columns repeat(auto-fit, minmax(180px, 1fr)), gap 16px, margin 24px 0
- `.stat-card` — padding 16px, background var(--color-surface), border-radius 8px
- `.stat-label` — font-size 12px, color var(--color-text-secondary), text-transform uppercase, letter-spacing 0.05em, margin-bottom 4px
- `.stat-value` — font-size 18px, font-weight 600
- `.chart-container` — margin 24px 0, position relative

Modal classes:
- `.modal-overlay` — position fixed, inset 0, background rgba(0,0,0,0.5), display flex, align-items center, justify-content center, z-index 1000
- `.modal-content` — background white, border-radius 12px, padding 24px, max-width 480px, width 100%, max-height 90vh, overflow-y auto
- `.modal-header` — display flex, justify-content space-between, align-items center, margin-bottom 20px
- `.modal-title` — font-size 18px, font-weight 600
- `.modal-close` — background none, border none, font-size 20px, cursor pointer, color var(--color-text-secondary), padding 4px

Utility classes:
- `.text-green` — color var(--color-green)
- `.text-red` — color var(--color-red)
- `.text-secondary` — color var(--color-text-secondary)
- `.loading` — display flex, align-items center, justify-content center, padding 60px, color var(--color-text-secondary)
- `.empty-state` — text-align center, padding 60px 20px, color var(--color-text-secondary)
- `.empty-state p` — font-size 14px, margin-top 8px

Responsive: at max-width 768px, stack layout, reduce padding, table becomes horizontally scrollable with overflow-x auto on a wrapper div. Hide less important table columns on mobile.

No gradients. No emojis. No purple. No custom fonts. Minimal, clean, Linear.app-inspired."
deps = []
model_tier = "fast"
tool_budget = 25
files_hint = ["src/index.css"]

[[task]]
id = "t5"
title = "Create useCryptoData hook"
description = "Create src/hooks/useCryptoData.ts — a React hook for fetching cryptocurrency market data with auto-refresh.

The hook should import from these existing files:
- `import { getMarkets, getCoinDetail, getMarketChart } from '../api/coingecko'`
- Types are in `src/types/coingecko.ts` — the key types are CoinMarket (with id, symbol, name, image, current_price, market_cap, market_cap_rank, price_change_percentage_24h, sparkline_in_7d.price, total_volume) and CoinDetail.

Create and export THREE hooks:

### 1. `useMarkets(perPage?: number, page?: number)`
```
Returns: { coins: CoinMarket[], loading: boolean, error: string | null, refetch: () => void }
```
- Calls getMarkets(perPage || 50, page || 1) on mount
- Auto-refreshes every 60 seconds using setInterval
- Stores data in state, tracks loading and error states
- refetch() triggers an immediate re-fetch
- Cleanup interval on unmount
- Use try/catch for error handling

### 2. `useCoinDetail(coinId: string | null)`
```
Returns: { coin: CoinDetail | null, loading: boolean, error: string | null }
```
- Calls getCoinDetail(coinId) when coinId changes and is not null
- Loading state toggles on fetch start/end
- Error state captures message

### 3. `useMarketChart(coinId: string | null, days?: number)`
```
Returns: { chartData: { prices: [number, number][] } | null, loading: boolean, error: string | null }
```
- Calls getMarketChart(coinId, days || 7) when coinId or days change
- Same loading/error pattern

All hooks use useState and useEffect from React. Use proper TypeScript generics for state types. Include proper cleanup (AbortController or flag-based cancellation to avoid state updates after unmount).

IMPORTANT: If the existing API functions have different signatures than what I described, adapt the calls to match. The hook names and return shapes should be exactly as specified."
deps = []
model_tier = "fast"
tool_budget = 25
files_hint = ["src/hooks/useCryptoData.ts"]

[[task]]
id = "t6"
title = "Create usePortfolio hook"
description = "Create src/hooks/usePortfolio.ts — a React hook for managing a crypto portfolio persisted to localStorage.

Define this type locally (export it):
```typescript
export interface PortfolioHolding {
  id: string;
  coinId: string;
  coinName: string;
  coinSymbol: string;
  amount: number;
  buyPrice: number; // USD price per coin at time of purchase
  addedAt: string; // ISO date string
}
```

Export a single hook: `usePortfolio()`

Returns:
```typescript
{
  holdings: PortfolioHolding[];
  addHolding: (holding: Omit<PortfolioHolding, 'id' | 'addedAt'>) => void;
  removeHolding: (id: string) => void;
  updateHolding: (id: string, updates: Partial<Pick<PortfolioHolding, 'amount' | 'buyPrice'>>) => void;
  totalValue: (currentPrices: Record<string, number>) => number;
  totalCost: () => number;
  totalPnL: (currentPrices: Record<string, number>) => { value: number; percent: number };
  holdingPnL: (holding: PortfolioHolding, currentPrice: number) => { value: number; percent: number };
}
```

Implementation:
- Load holdings from localStorage key 'crypto-portfolio' on mount (parse JSON, default to [])
- Save to localStorage whenever holdings change (use useEffect watching holdings)
- addHolding: generate id with crypto.randomUUID() or Date.now().toString(36) + random, set addedAt to new Date().toISOString()
- removeHolding: filter by id
- updateHolding: find by id and merge updates
- totalValue: sum of (amount * currentPrice) for each holding, using currentPrices[coinId]
- totalCost: sum of (amount * buyPrice) for each holding
- totalPnL: value = totalValue - totalCost, percent = (value / totalCost) * 100 (handle division by zero)
- holdingPnL: value = (amount * currentPrice) - (amount * buyPrice), percent = ((currentPrice - buyPrice) / buyPrice) * 100

Use useState, useEffect, useCallback from React. TypeScript with proper types. Handle JSON parse errors gracefully."
deps = []
model_tier = "fast"
tool_budget = 25
files_hint = ["src/hooks/usePortfolio.ts"]

[[task]]
id = "t7"
title = "Create PriceTable component"
description = "Create src/components/PriceTable.tsx — a searchable cryptocurrency price table.

Imports:
```
import { CoinMarket } from '../types/coingecko'
import { formatCurrency, formatPercent, formatCompact } from '../utils/format'
import Sparkline from './Sparkline'
```

Props:
```typescript
interface PriceTableProps {
  coins: CoinMarket[];
  onSelectCoin: (coinId: string) => void;
}
```

Implementation:
- Search bar at top: text input with placeholder 'Search coins...', filters coins by name or symbol (case-insensitive), use local state for search term
- Table with columns: #, Coin, Price, 24h %, Market Cap, Volume (24h), Last 7 Days
- Each row is clickable (calls onSelectCoin with coin.id)
- Column details:
  - #: coin.market_cap_rank, styled with .rank class
  - Coin: image (32x32, border-radius 50%), name, and symbol in a flex layout (.coin-info, .coin-image, .coin-name, .coin-symbol)
  - Price: formatCurrency(coin.current_price)
  - 24h %: formatPercent(coin.price_change_percentage_24h), colored green (.positive) if >= 0, red (.negative) if < 0
  - Market Cap: formatCompact(coin.market_cap)
  - Volume: formatCompact(coin.total_volume)
  - Sparkline: <Sparkline data={coin.sparkline_in_7d.price} width={120} height={40} />
- Wrap table in a div with overflow-x auto for mobile
- Use CSS classes: .search-input, .price-table, .positive, .negative, .rank, .coin-info, .coin-image, .coin-name, .coin-symbol
- Show empty state if no coins match search
- Loading state if coins array is empty (show 'Loading...' centered)

Export default. TypeScript React functional component."
deps = ["t1", "t2"]
model_tier = "fast"
tool_budget = 25
files_hint = ["src/components/PriceTable.tsx"]

[[task]]
id = "t8"
title = "Create CoinDetail component"
description = "Create src/components/CoinDetail.tsx — detailed view for a single cryptocurrency with a Chart.js line chart.

Imports:
```
import { useCoinDetail, useMarketChart } from '../hooks/useCryptoData'
import { formatCurrency, formatPercent, formatCompact, formatNumber } from '../utils/format'
import { Line } from 'react-chartsjs-2' // Note: the project should have chart.js and react-chartjs-2 installed
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  PointElement,
  LineElement,
  Tooltip,
  Filler,
} from 'chart.js'
```
Register ChartJS components: CategoryScale, LinearScale, PointElement, LineElement, Tooltip, Filler.

If chart.js is not installed, the component should still render but show the stats without the chart. Use a try/catch or check if Line is available. Actually — just import it normally and let the build handle it. The project likely has it in package.json already since this is a crypto dashboard.

Props:
```typescript
interface CoinDetailProps {
  coinId: string;
  onBack: () => void;
}
```

Implementation:
- Call useCoinDetail(coinId) and useMarketChart(coinId)
- Back button at top: class .back-btn, onClick onBack, text '← Back'
- Header section: coin image (48x48), coin name and symbol, current price large (.detail-price)
- 24h change: formatPercent, colored green/red

- Stats grid (.detail-stats) with stat cards (.stat-card):
  - Market Cap: formatCompact(coin.market_data.market_cap.usd)
  - 24h Volume: formatCompact(coin.market_data.total_volume.usd)
  - Circulating Supply: formatNumber(coin.market_data.circulating_supply) + ' ' + symbol
  - All-Time High: formatCurrency(coin.market_data.ath.usd)
  - All-Time Low: formatCurrency(coin.market_data.atl.usd)
  - 24h High / Low: formatCurrency(coin.market_data.high_24h.usd) / formatCurrency(coin.market_data.low_24h.usd)

- Chart section (.chart-container):
  - Day range selector buttons: 1D, 7D, 30D, 90D, 1Y (use local state for selected range, pass days to useMarketChart)
  - Chart.js Line chart:
    - Data: timestamps as labels (format as short date), prices as dataset
    - Options: no legend, responsive true, maintainAspectRatio false, height ~400px
    - Line color: #2563EB, fill with very light blue gradient (rgba(37,99,235,0.08))
    - No point dots (pointRadius: 0), lineWidth 2
    - Tooltip showing formatted currency
    - Y-axis: format as currency
    - X-axis: show date labels, maxTicksLimit 8

- Description section: coin.description.en (render as HTML with dangerouslySetInnerHTML, or strip HTML tags and show as text)

- Loading state: show 'Loading...' centered
- Error state: show error message

Use .text-green, .text-red, .text-secondary utility classes. Export default."
deps = ["t1", "t5"]
model_tier = "fast"
tool_budget = 30
files_hint = ["src/components/CoinDetail.tsx"]

[[task]]
id = "t9"
title = "Create Portfolio component"
description = "Create src/components/Portfolio.tsx — portfolio tracker showing holdings and P&L.

Imports:
```
import { usePortfolio, PortfolioHolding } from '../hooks/usePortfolio'
import { formatCurrency, formatPercent } from '../utils/format'
import { CoinMarket } from '../types/coingecko'
import AddHoldingModal from './AddHoldingModal'
```

Props:
```typescript
interface PortfolioProps {
  coins: CoinMarket[]; // current market data to calculate current values
}
```

Implementation:
- Use usePortfolio() hook
- Build a currentPrices map from coins: Record<string, number> mapping coin.id → coin.current_price

- Header section (.portfolio-header):
  - Left side:
    - 'Portfolio' title (h2)
    - Total value: formatCurrency(totalValue(currentPrices)) — large text (.portfolio-total)
    - Total P&L: show value and percent, colored green/red (.portfolio-change, .text-green or .text-red)
    - Total cost basis: 'Cost basis: ' + formatCurrency(totalCost()) in .text-secondary
  - Right side:
    - 'Add Holding' button (.btn .btn-primary) that opens the AddHoldingModal

- Holdings list (.holdings-list):
  - If empty: show empty state — 'No holdings yet. Add your first crypto holding to start tracking.' with the Add button
  - For each holding, render a .holding-item:
    - Left (.holding-info):
      - Coin image from coins.find(c => c.id === holding.coinId)?.image (32x32, border-radius 50%), fallback to a colored circle with first letter
      - Holding amount + ' ' + symbol (.coin-name style)
      - 'Bought at ' + formatCurrency(holding.buyPrice) (.text-secondary)
    - Right (.holding-values):
      - Current value: formatCurrency(amount * currentPrice)
      - P&L (.holding-pnl): formatCurrency(pnl.value) + ' (' + formatPercent(pnl.percent) + ')', colored green/red
    - Delete button: small X or trash icon button on the far right, calls removeHolding(holding.id)

- Modal state: local boolean showAddModal, toggle on button click
- When modal's onAdd fires: call addHolding, close modal

- Use the CSS classes: .portfolio-header, .portfolio-total, .portfolio-change, .holdings-list, .holding-item, .holding-info, .holding-values, .holding-pnl, .btn, .btn-primary, .empty-state, .text-green, .text-red, .text-secondary

Export default. TypeScript React FC."
deps = ["t1", "t3", "t6"]
model_tier = "fast"
tool_budget = 30
files_hint = ["src/components/Portfolio.tsx"]

[[task]]
id = "t10"
title = "Create main App component"
description = "REPLACE src/App.tsx with the main application component that wires everything together.

Imports:
```
import { useState } from 'react'
import { useMarkets } from './hooks/useCryptoData'
import PriceTable from './components/PriceTable'
import CoinDetail from './components/CoinDetail'
import Portfolio from './components/Portfolio'
```

Types:
```typescript
type View = 'markets' | 'portfolio'
```

Implementation:
- Local state: view (View, default 'markets'), selectedCoinId (string | null, default null)
- Call useMarkets(100) to fetch top 100 coins

- Layout structure:
```
<div className="app">
  <header className="header">
    <h1>Crypto Dashboard</h1>
    <nav className="nav-tabs">
      <button className={`nav-tab ${view === 'markets' && !selectedCoinId ? 'active' : ''}`} onClick={() => { setView('markets'); setSelectedCoinId(null); }}>Markets</button>
      <button className={`nav-tab ${view === 'portfolio' ? 'active' : ''}`} onClick={() => { setView('portfolio'); setSelectedCoinId(null); }}>Portfolio</button>
    </nav>
  </header>

  <main>
    {selectedCoinId ? (
      <CoinDetail coinId={selectedCoinId} onBack={() => setSelectedCoinId(null)} />
    ) : view === 'markets' ? (
      <PriceTable coins={coins} onSelectCoin={setSelectedCoinId} />
    ) : (
      <Portfolio coins={coins} />
    )}
  </main>
</div>
```

- Show loading state when loading && coins.length === 0
- Show error state when error && coins.length === 0

The component should be clean, minimal, and properly typed. Use CSS classes: .app, .header, .nav-tabs, .nav-tab, .nav-tab.active, .loading, .empty-state.

Export default App."
deps = ["t4", "t7", "t8", "t9"]
model_tier = "fast"
tool_budget = 25
files_hint = ["src/App.tsx"]
</plan>