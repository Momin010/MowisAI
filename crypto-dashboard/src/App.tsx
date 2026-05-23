import { useState } from 'react'
import { useCryptoData } from './hooks/useCryptoData'
import PriceTable from './components/PriceTable'
import CoinDetail from './components/CoinDetail'
import Portfolio from './components/Portfolio'

type View = 'markets' | 'portfolio'

export default function App() {
  const [view, setView] = useState<View>('markets')
  const [selectedCoinId, setSelectedCoinId] = useState<string | null>(null)
  const { coins, isLoading, error } = useCryptoData({ perPage: 100 })

  return (
    <div className="app">
      <header className="header">
        <h1>Crypto Dashboard</h1>
        <nav className="nav-tabs">
          <button
            className={`nav-tab ${view === 'markets' && !selectedCoinId ? 'active' : ''}`}
            onClick={() => { setView('markets'); setSelectedCoinId(null) }}
          >
            Markets
          </button>
          <button
            className={`nav-tab ${view === 'portfolio' ? 'active' : ''}`}
            onClick={() => { setView('portfolio'); setSelectedCoinId(null) }}
          >
            Portfolio
          </button>
        </nav>
      </header>

      <main>
        {selectedCoinId ? (
          <CoinDetail coinId={selectedCoinId} onBack={() => setSelectedCoinId(null)} coins={coins} />
        ) : view === 'markets' ? (
          isLoading && coins.length === 0 ? (
            <div className="loading">Loading market data...</div>
          ) : error ? (
            <div className="empty-state"><p>Error: {error}</p></div>
          ) : (
            <PriceTable coins={coins} onSelectCoin={setSelectedCoinId} />
          )
        ) : (
          <Portfolio coins={coins} />
        )}
      </main>
    </div>
  )
}
