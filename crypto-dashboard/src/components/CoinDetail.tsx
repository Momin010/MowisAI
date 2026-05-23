import { useState } from 'react'
import type { CryptoCoin } from '../hooks/useCryptoData'

interface CoinDetailProps {
  coinId: string
  onBack: () => void
  coins: CryptoCoin[]
}

export default function CoinDetail({ coinId, onBack, coins }: CoinDetailProps) {
  const [days, setDays] = useState(7)
  const coin = coins.find(c => c.id === coinId)

  if (!coin) return <div className="empty-state">Coin not found</div>

  const change24h = coin.priceChange24h ?? 0

  const timeframes = [
    { label: '1D', value: 1 },
    { label: '7D', value: 7 },
    { label: '30D', value: 30 },
    { label: '90D', value: 90 },
    { label: '1Y', value: 365 },
  ]

  return (
    <div>
      <button className="back-btn" onClick={onBack}>Back</button>

      <div className="detail-header">
        <img src={coin.image} alt={coin.name} width={48} height={48} style={{ borderRadius: '50%' }} />
        <div>
          <h2 className="detail-title">{coin.name} <span className="text-secondary">{coin.symbol.toUpperCase()}</span></h2>
          <div className="detail-price">${coin.currentPrice?.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}</div>
          <div className={change24h >= 0 ? 'text-green' : 'text-red'}>
            {change24h >= 0 ? '+' : ''}{change24h.toFixed(2)}%
          </div>
        </div>
      </div>

      <div className="detail-stats">
        <div className="stat-card">
          <div className="stat-label">Market Cap</div>
          <div className="stat-value">${formatCompact(coin.marketCap)}</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">24h Volume</div>
          <div className="stat-value">${formatCompact(coin.totalVolume)}</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Rank</div>
          <div className="stat-value">#{coin.marketCapRank}</div>
        </div>
      </div>

      <div className="chart-container">
        <div style={{ display: 'flex', gap: 4, marginBottom: 16 }}>
          {timeframes.map(tf => (
            <button
              key={tf.value}
              className={`btn ${days === tf.value ? 'btn-primary' : ''}`}
              onClick={() => setDays(tf.value)}
            >
              {tf.label}
            </button>
          ))}
        </div>
        <div className="empty-state">
          <p>Chart data will be available when connected to the API</p>
        </div>
      </div>
    </div>
  )
}

function formatCompact(n: number): string {
  if (n >= 1e12) return (n / 1e12).toFixed(2) + 'T'
  if (n >= 1e9) return (n / 1e9).toFixed(2) + 'B'
  if (n >= 1e6) return (n / 1e6).toFixed(2) + 'M'
  if (n >= 1e3) return (n / 1e3).toFixed(2) + 'K'
  return n.toFixed(2)
}
