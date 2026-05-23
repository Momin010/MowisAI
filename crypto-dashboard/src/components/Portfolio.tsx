import { useState } from 'react'
import { usePortfolio } from '../hooks/usePortfolio'
import type { CryptoCoin } from '../hooks/useCryptoData'
import AddHoldingModal from './AddHoldingModal'

interface PortfolioProps {
  coins: CryptoCoin[]
}

export default function Portfolio({ coins }: PortfolioProps) {
  const { holdings, addHolding, removeHolding } = usePortfolio()
  const [showModal, setShowModal] = useState(false)

  const currentPrices: Record<string, number> = {}
  coins.forEach(c => { currentPrices[c.id] = c.currentPrice })

  const totalValue = holdings.reduce((sum, h) => sum + h.amount * (currentPrices[h.symbol] ?? 0), 0)
  const totalCost = holdings.reduce((sum, h) => sum + h.amount * h.purchasePrice, 0)
  const pnlValue = totalValue - totalCost
  const pnlPercent = totalCost > 0 ? (pnlValue / totalCost) * 100 : 0

  return (
    <div>
      <div className="portfolio-header">
        <div>
          <h2>Portfolio</h2>
          <div className="portfolio-total">${totalValue.toLocaleString('en-US', { minimumFractionDigits: 2 })}</div>
          <div className={`portfolio-change ${pnlValue >= 0 ? 'text-green' : 'text-red'}`}>
            {pnlValue >= 0 ? '+' : ''}${pnlValue.toFixed(2)} ({pnlPercent.toFixed(2)}%)
          </div>
          <div className="text-secondary">Cost basis: ${totalCost.toFixed(2)}</div>
        </div>
        <button className="btn btn-primary" onClick={() => setShowModal(true)}>Add Holding</button>
      </div>

      {holdings.length === 0 ? (
        <div className="empty-state">
          <p>No holdings yet. Add your first crypto holding to start tracking.</p>
          <button className="btn btn-primary" onClick={() => setShowModal(true)} style={{ marginTop: 12 }}>Add Holding</button>
        </div>
      ) : (
        <div className="holdings-list">
          {holdings.map(h => {
            const currentPrice = currentPrices[h.symbol] ?? 0
            const holdingValue = h.amount * currentPrice
            const holdingPnlVal = (currentPrice - h.purchasePrice) * h.amount
            const holdingPnlPct = h.purchasePrice > 0 ? ((currentPrice - h.purchasePrice) / h.purchasePrice) * 100 : 0
            const coin = coins.find(c => c.symbol.toLowerCase() === h.symbol.toLowerCase())

            return (
              <div key={h.id} className="holding-item">
                <div className="holding-info">
                  {coin?.image && <img src={coin.image} alt={h.name} width={32} height={32} style={{ borderRadius: '50%' }} />}
                  <div>
                    <div className="coin-name">{h.amount} {h.symbol.toUpperCase()}</div>
                    <div className="text-secondary">Bought at ${h.purchasePrice.toFixed(2)}</div>
                  </div>
                </div>
                <div className="holding-values">
                  <div>${holdingValue.toFixed(2)}</div>
                  <div className={`holding-pnl ${holdingPnlVal >= 0 ? 'text-green' : 'text-red'}`}>
                    {holdingPnlVal >= 0 ? '+' : ''}${holdingPnlVal.toFixed(2)} ({holdingPnlPct.toFixed(2)}%)
                  </div>
                </div>
                <button className="btn" onClick={() => removeHolding(h.id)} style={{ marginLeft: 12, color: '#EF4444' }}>X</button>
              </div>
            )
          })}
        </div>
      )}

      {showModal && (
        <AddHoldingModal
          isOpen={showModal}
          onClose={() => setShowModal(false)}
          onSubmit={(data) => {
            addHolding({ symbol: data.coinSymbol, name: data.coinName, amount: data.quantity, purchasePrice: data.purchasePrice ?? 0 })
            setShowModal(false)
          }}
        />
      )}
    </div>
  )
}
