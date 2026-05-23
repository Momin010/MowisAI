/* eslint-disable @typescript-eslint/no-explicit-any */

// ---------------------------------------------------------------------------
// CoinGecko API type definitions
// ---------------------------------------------------------------------------

/** Generic API response wrapper */
export interface ApiResponse<T> {
  success: boolean;
  data: T;
  error?: string;
}

/** Coin listed in /coins/markets */
export interface CoinMarket {
  id: string;
  symbol: string;
  name: string;
  image: string;
  current_price: number;
  market_cap: number;
  market_cap_rank: number | null;
  fully_diluted_valuation: number | null;
  total_volume: number;
  high_24h: number;
  low_24h: number;
  price_change_24h: number;
  price_change_percentage_24h: number;
  market_cap_change_24h: number;
  market_cap_change_percentage_24h: number;
  circulating_supply: number;
  total_supply: number | null;
  max_supply: number | null;
  ath: number;
  ath_change_percentage: number;
  ath_date: string;
  atl: number;
  atl_change_percentage: number;
  atl_date: string;
  roi: { times: number; currency: string; percentage: number } | null;
  last_updated: string;
  sparkline_in_7d?: { price: number[] };
  price_change_percentage_24h_in_currency?: number;
}

/** Detailed coin info from /coins/{id} */
export interface CoinDetail {
  id: string;
  symbol: string;
  name: string;
  description: { en: string };
  links: {
    homepage: string[];
    blockchain_site: string[];
    official_forum_url: string[];
    subreddit_url: string;
    repos_url: { github: string[]; bitbucket: string[] };
  };
  image: { thumb: string; small: string; large: string };
  market_cap_rank: number | null;
  market_data: {
    current_price: Record<string, number>;
    market_cap: Record<string, number>;
    total_volume: Record<string, number>;
    high_24h: Record<string, number>;
    low_24h: Record<string, number>;
    price_change_percentage_24h: number;
    price_change_percentage_7d: number;
    price_change_percentage_30d: number;
    price_change_percentage_1y: number;
    circulating_supply: number;
    total_supply: number | null;
    max_supply: number | null;
  };
}

/** Response from /simple/price */
export interface CoinPrice {
  [coinId: string]: {
    [currency: string]: number;
  };
}

/** Response from /coins/{id}/market_chart */
export interface MarketChart {
  prices: [number, number][];
  market_caps: [number, number][];
  total_volumes: [number, number][];
}

/** A trending coin entry */
export interface TrendingCoin {
  item: {
    id: string;
    coin_id: number;
    name: string;
    symbol: string;
    market_cap_rank: number | null;
    thumb: string;
    small: string;
    large: string;
    slug: string;
    price_btc: number;
    score: number;
  };
}
