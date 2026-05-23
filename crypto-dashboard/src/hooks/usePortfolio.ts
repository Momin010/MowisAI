import { useState, useCallback, useEffect } from 'react';

const STORAGE_KEY = 'crypto-portfolio';

export interface PortfolioHolding {
  id: string;
  symbol: string;
  name: string;
  amount: number;
  purchasePrice: number;
  addedAt: string;
}

interface UsePortfolioReturn {
  holdings: PortfolioHolding[];
  addHolding: (holding: Omit<PortfolioHolding, 'id' | 'addedAt'>) => void;
  removeHolding: (id: string) => void;
  updateHolding: (id: string, updates: Partial<Pick<PortfolioHolding, 'amount' | 'purchasePrice'>>) => void;
  clearPortfolio: () => void;
  isLoading: boolean;
  error: string | null;
}

function generateId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

function loadFromStorage(): PortfolioHolding[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];

    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];

    // Validate each entry has required fields
    return parsed.filter(
      (item): item is PortfolioHolding =>
        typeof item === 'object' &&
        item !== null &&
        typeof item.id === 'string' &&
        typeof item.symbol === 'string' &&
        typeof item.name === 'string' &&
        typeof item.amount === 'number' &&
        typeof item.purchasePrice === 'number' &&
        typeof item.addedAt === 'string'
    );
  } catch {
    return [];
  }
}

function saveToStorage(holdings: PortfolioHolding[]): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(holdings));
  } catch {
    // Storage full or unavailable — silently fail
  }
}

export function usePortfolio(): UsePortfolioReturn {
  const [holdings, setHoldings] = useState<PortfolioHolding[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Load from localStorage on mount
  useEffect(() => {
    try {
      const stored = loadFromStorage();
      setHoldings(stored);
      setError(null);
    } catch (err) {
      setError('Failed to load portfolio from storage.');
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Persist whenever holdings change (skip the initial load)
  useEffect(() => {
    if (!isLoading) {
      saveToStorage(holdings);
    }
  }, [holdings, isLoading]);

  const addHolding = useCallback(
    (holding: Omit<PortfolioHolding, 'id' | 'addedAt'>) => {
      const { symbol, name, amount, purchasePrice } = holding;

      if (!symbol.trim()) {
        setError('Symbol is required.');
        return;
      }
      if (!name.trim()) {
        setError('Name is required.');
        return;
      }
      if (amount <= 0 || !Number.isFinite(amount)) {
        setError('Amount must be a positive number.');
        return;
      }
      if (purchasePrice < 0 || !Number.isFinite(purchasePrice)) {
        setError('Purchase price must be a non-negative number.');
        return;
      }

      const newHolding: PortfolioHolding = {
        id: generateId(),
        symbol: symbol.trim().toUpperCase(),
        name: name.trim(),
        amount,
        purchasePrice,
        addedAt: new Date().toISOString(),
      };

      setHoldings((prev) => [...prev, newHolding]);
      setError(null);
    },
    []
  );

  const removeHolding = useCallback((id: string) => {
    if (!id.trim()) {
      setError('Invalid holding ID.');
      return;
    }
    setHoldings((prev) => prev.filter((h) => h.id !== id));
    setError(null);
  }, []);

  const updateHolding = useCallback(
    (id: string, updates: Partial<Pick<PortfolioHolding, 'amount' | 'purchasePrice'>>) => {
      if (!id.trim()) {
        setError('Invalid holding ID.');
        return;
      }

      if (updates.amount !== undefined && (updates.amount <= 0 || !Number.isFinite(updates.amount))) {
        setError('Amount must be a positive number.');
        return;
      }

      if (
        updates.purchasePrice !== undefined &&
        (updates.purchasePrice < 0 || !Number.isFinite(updates.purchasePrice))
      ) {
        setError('Purchase price must be a non-negative number.');
        return;
      }

      setHoldings((prev) =>
        prev.map((h) =>
          h.id === id ? { ...h, ...updates } : h
        )
      );
      setError(null);
    },
    []
  );

  const clearPortfolio = useCallback(() => {
    setHoldings([]);
    setError(null);
  }, []);

  return {
    holdings,
    addHolding,
    removeHolding,
    updateHolding,
    clearPortfolio,
    isLoading,
    error,
  };
}

export default usePortfolio;
