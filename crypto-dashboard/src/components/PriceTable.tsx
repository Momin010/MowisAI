import { useState } from 'react'
import type { CryptoCoin } from '../hooks/useCryptoData'
import Sparkline from './Sparkline'

interface PriceTableProps {
  coins: CryptoCoin[]
  onSelectCoin: (coinId: string) => void
}

export default function PriceTable({ coins, onSelectCoin }: PriceTableProps) {
  const [search, setSearch] = useState('')

  const filtered = coins.filter(c =>
    c.name.toLowerCase().includes(search.toLowerCase()) ||
    c.symbol.toLowerCase().includes(search.toLowerCase())
  )

  return (
    <div className="card">
      <input
        className="search-input"
        placeholder="Search coins..."
        value={search}
        onChange={e => setSearch(e.target.value)}
      />
      <div style={{ overflowX: 'auto' }}>
        <table className="price-table">
          <thead>
            <tr>
              <th>#</th>
              <th>Coin</th>
              <th>Price</th>
              <th>24h %</th>
              <th>Market Cap</th>
              <th>Volume</th>
              <th>7d</th>
            </tr>
          </thead>
          <tbody>
            {filtered.length === 0 ? (
              <tr><td colSpan={7} style={{ textAlign: 'center', padding: 40, color: '#6B7280' }}>
                {coins.length === 0 ? 'Loading...' : 'No coins found'}
              </td></tr>
            ) : filtered.map(coin => {
              const change = coin.priceChange24h ?? 0
              const sparkData: number[] = []
              return (
                <tr key={coin.id} onClick={() => onSelectCoin(coin.id)}>
                  <td className="rank">{coin.marketCapRank}</td>
                  <td>
                    <div className="coin-info">
                      <img className="coin-image" src={coin.image} alt={coin.name} />
                      <span className="coin-name">{coin.name}</span>
                      <span className="coin-symbol">{coin.symbol}</span>
                    </div>
                  </td>
                  <td>${coin.currentPrice?.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}</td>
                  <td className={change >= 0 ? 'positive' : 'negative'}>
                    {change >= 0 ? '+' : ''}{change.toFixed(2)}%
                  </td>
                  <td>${formatCompact(coin.marketCap)}</td>
                  <td>${formatCompact(coin.totalVolume)}</td>
                  <td><Sparkline data={sparkData} width={120} height={32} /></td>
                </tr>
              )
            })}
          </tbody>
        </table>
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
