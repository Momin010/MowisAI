import { useState, useEffect, useCallback, useRef } from 'react';
import { coingecko, CoinGeckoError } from '../api/coingecko';
import type { CoinMarket } from '../types/coingecko';

/**
 * CoinGecko API — free, no API key required.
 * Uses the shared service layer for consistent error handling.
 */

const DEFAULT_REFRESH_INTERVAL_MS = 60_000; // 1 minute

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface CryptoCoin {
  id: string;
  symbol: string;
  name: string;
  image: string;
  currentPrice: number;
  marketCap: number;
  marketCapRank: number | null;
  totalVolume: number;
  high24h: number;
  low24h: number;
  priceChange24h: number;
  priceChangePercentage24h: number;
  circulatingSupply: number;
  totalSupply: number | null;
  lastUpdated: string;
}

export interface UseCryptoDataOptions {
  /** Number of coins to fetch (default 20) */
  perPage?: number;
  /** Currency for prices (default 'usd') */
  vsCurrency?: string;
  /** Auto-refresh interval in ms (default 60 000). Set to 0 to disable. */
  refreshInterval?: number;
  /** Sort key (default 'market_cap_desc') */
  orderBy?: 'market_cap_desc' | 'market_cap_asc' | 'volume_desc' | 'volume_asc' | 'id_asc' | 'id_desc';
}

export interface UseCryptoDataResult {
  coins: CryptoCoin[];
  isLoading: boolean;
  error: string | null;
  lastUpdated: Date | null;
  refresh: () => void;
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

const DEFAULT_OPTIONS: Required<UseCryptoDataOptions> = {
  perPage: 20,
  vsCurrency: 'usd',
  refreshInterval: DEFAULT_REFRESH_INTERVAL_MS,
  orderBy: 'market_cap_desc',
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Maps raw CoinGecko CoinMarket to our clean camelCase interface.
 */
function mapCoin(raw: CoinMarket): CryptoCoin {
  return {
    id: raw.id,
    symbol: raw.symbol,
    name: raw.name,
    image: raw.image,
    currentPrice: raw.current_price,
    marketCap: raw.market_cap,
    marketCapRank: raw.market_cap_rank,
    totalVolume: raw.total_volume,
    high24h: raw.high_24h,
    low24h: raw.low_24h,
    priceChange24h: raw.price_change_24h,
    priceChangePercentage24h: raw.price_change_percentage_24h,
    circulatingSupply: raw.circulating_supply,
    totalSupply: raw.total_supply,
    lastUpdated: raw.last_updated,
  };
}

// ---------------------------------------------------------------------------
// Hook
// ---------------------------------------------------------------------------

/**
 * Fetches cryptocurrency market data from CoinGecko with automatic
 * refresh support. No API key required.
 *
 * @example
 * ```tsx
 * const { coins, isLoading, error, refresh } = useCryptoData({ perPage: 10 });
 *
 * if (isLoading && coins.length === 0) return <Spinner />;
 * if (error) return <ErrorMessage message={error} />;
 *
 * return (
 *   <ul>
 *     {coins.map(coin => (
 *       <li key={coin.id}>{coin.name}: ${coin.currentPrice}</li>
 *     ))}
 *   </ul>
 * );
 * ```
 */
export function useCryptoData(options?: UseCryptoDataOptions): UseCryptoDataResult {
  const opts = { ...DEFAULT_OPTIONS, ...options };

  const [coins, setCoins] = useState<CryptoCoin[]>([]);
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [error, setError] = useState<string | null>(null);
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);

  // Track whether we have data to avoid flickering isLoading on refresh.
  const hasDataRef = useRef(false);

  // Guard against state updates after unmount.
  const isMountedRef = useRef(true);

  useEffect(() => {
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  const fetchData = useCallback(async () => {
    // Only flash loading spinner on the initial fetch (no data yet).
    if (!hasDataRef.current) {
      setIsLoading(true);
    }
    setError(null);

    try {
      const response = await coingecko.getMarkets({
        vsCurrency: opts.vsCurrency,
        order: opts.orderBy,
        perPage: opts.perPage,
        page: 1,
        sparkline: false,
      });

      if (!response.success) {
        throw new Error(response.error ?? 'Failed to fetch market data');
      }

      if (!isMountedRef.current) return;

      const mapped = response.data.map(mapCoin);
      setCoins(mapped);
      setLastUpdated(new Date());
      setError(null);
      hasDataRef.current = true;
    } catch (err: unknown) {
      if (!isMountedRef.current) return;

      if (err instanceof CoinGeckoError) {
        setError(err.message);
      } else if (err instanceof Error) {
        setError(err.message);
      } else {
        setError('An unknown error occurred while fetching crypto data');
      }
    } finally {
      if (isMountedRef.current) {
        setIsLoading(false);
      }
    }
  }, [opts.vsCurrency, opts.orderBy, opts.perPage]);

  // Initial fetch + auto-refresh interval.
  useEffect(() => {
    fetchData();

    if (opts.refreshInterval > 0) {
      const intervalId = setInterval(fetchData, opts.refreshInterval);
      return () => clearInterval(intervalId);
    }
  }, [fetchData, opts.refreshInterval]);

  const refresh = useCallback(() => {
    fetchData();
  }, [fetchData]);

  return { coins, isLoading, error, lastUpdated, refresh };
}

export default useCryptoData;
