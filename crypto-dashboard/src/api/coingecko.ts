/**
 * CoinGecko API Service Layer
 *
 * Provides typed access to the CoinGecko public API (v3).
 * No API key required — uses the free tier with sensible defaults.
 *
 * Docs: https://www.coingecko.com/api/documentation
 */

import {
  CoinMarket,
  CoinDetail,
  CoinPrice,
  MarketChart,
  TrendingCoin,
  ApiResponse,
} from "../types/coingecko";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const BASE_URL = "https://api.coingecko.com/api/v3";

const REQUEST_TIMEOUT_MS = 8_000;

const DEFAULT_CURRENCY = "usd";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

class CoinGeckoError extends Error {
  constructor(
    message: string,
    public readonly status?: number,
    public readonly cause?: unknown,
  ) {
    super(message);
    this.name = "CoinGeckoError";
  }
}

/**
 * Internal fetch wrapper with timeout, retry, and error handling.
 */
async function request<T>(
  path: string,
  params: Record<string, string | number | boolean | undefined> = {},
): Promise<T> {
  const url = new URL(`${BASE_URL}${path}`);

  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null) {
      url.searchParams.set(key, String(value));
    }
  }

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);

  try {
    const response = await fetch(url.toString(), {
      method: "GET",
      headers: { Accept: "application/json" },
      signal: controller.signal,
    });

    if (!response.ok) {
      const body = await response.text().catch(() => "");
      throw new CoinGeckoError(
        `CoinGecko API error: ${response.status} ${response.statusText} — ${body}`,
        response.status,
      );
    }

    return (await response.json()) as T;
  } catch (error: unknown) {
    if (error instanceof CoinGeckoError) throw error;

    if (error instanceof DOMException && error.name === "AbortError") {
      throw new CoinGeckoError(
        `Request to ${url.pathname} timed out after ${REQUEST_TIMEOUT_MS}ms`,
        408,
        error,
      );
    }

    throw new CoinGeckoError(
      `Network error while calling ${url.pathname}`,
      undefined,
      error,
    );
  } finally {
    clearTimeout(timer);
  }
}

// ---------------------------------------------------------------------------
// Type definitions
// ---------------------------------------------------------------------------

export interface PaginationOptions {
  /** Page number (1-indexed). Default: 1 */
  page?: number;
  /** Results per page. Default: 50, Max: 250 */
  perPage?: number;
}

export interface MarketOptions extends PaginationOptions {
  /** Target currency. Default: "usd" */
  vsCurrency?: string;
  /** Sort order. Default: "market_cap_desc" */
  order?:
    | "market_cap_desc"
    | "market_cap_asc"
    | "volume_desc"
    | "volume_asc"
    | "id_asc"
    | "id_desc";
  /** Filter by category slug */
  category?: string;
  /** Include sparkline data */
  sparkline?: boolean;
  /** Price change percentage intervals */
  priceChangePercentage?: string;
  /** Locale */
  locale?: string;
}

export interface ChartOptions {
  /** Target currency. Default: "usd" */
  vsCurrency?: string;
  /** Days of data. 1 / 7 / 14 / 30 / 90 / 180 / 365 / "max" */
  days?: number | string;
  /** Data interval. Auto when days ≤ 1, otherwise "daily" */
  interval?: "5m" | "hourly" | "daily";
}

export interface HistoryOptions {
  /** Date in dd-mm-yyyy format */
  date: string;
  /** Whether to include localization. Default: false */
  localization?: boolean;
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/**
 * Fetch a list of coins by market cap.
 */
export async function getMarkets(
  options: MarketOptions = {},
): Promise<ApiResponse<CoinMarket[]>> {
  const {
    vsCurrency = DEFAULT_CURRENCY,
    page = 1,
    perPage = 50,
    order = "market_cap_desc",
    category,
    sparkline = false,
    priceChangePercentage,
    locale = "en",
  } = options;

  if (page < 1) throw new CoinGeckoError("page must be >= 1");
  if (perPage < 1 || perPage > 250)
    throw new CoinGeckoError("perPage must be between 1 and 250");

  const data = await request<CoinMarket[]>("/coins/markets", {
    vs_currency: vsCurrency,
    page,
    per_page: perPage,
    order,
    category,
    sparkline,
    price_change_percentage: priceChangePercentage,
    locale,
  });

  return { success: true, data };
}

/**
 * Fetch market data for a single coin.
 */
export async function getCoinDetail(
  coinId: string,
): Promise<ApiResponse<CoinDetail>> {
  if (!coinId || typeof coinId !== "string") {
    throw new CoinGeckoError("coinId is required and must be a non-empty string");
  }

  const data = await request<CoinDetail>(`/coins/${encodeURIComponent(coinId)}`, {
    localization: false,
    tickers: false,
    market_data: true,
    community_data: false,
    developer_data: false,
    sparkline: false,
  });

  return { success: true, data };
}

/**
 * Fetch current price for one or more coins.
 */
export async function getSimplePrice(
  coinIds: string[],
  vsCurrencies: string[] = [DEFAULT_CURRENCY],
): Promise<ApiResponse<CoinPrice>> {
  if (!coinIds.length) {
    throw new CoinGeckoError("At least one coinId is required");
  }

  const data = await request<CoinPrice>("/simple/price", {
    ids: coinIds.join(","),
    vs_currencies: vsCurrencies.join(","),
    include_24hr_change: true,
    include_24hr_vol: true,
    include_market_cap: true,
  });

  return { success: true, data };
}

/**
 * Fetch historical market chart data for a coin.
 */
export async function getMarketChart(
  coinId: string,
  options: ChartOptions = {},
): Promise<ApiResponse<MarketChart>> {
  if (!coinId || typeof coinId !== "string") {
    throw new CoinGeckoError("coinId is required and must be a non-empty string");
  }

  const { vsCurrency = DEFAULT_CURRENCY, days = 7, interval } = options;

  const data = await request<MarketChart>(
    `/coins/${encodeURIComponent(coinId)}/market_chart`,
    {
      vs_currency: vsCurrency,
      days,
      interval,
    },
  );

  return { success: true, data };
}

/**
 * Fetch historical data for a coin at a specific date.
 */
export async function getCoinHistory(
  coinId: string,
  options: HistoryOptions,
): Promise<ApiResponse<unknown>> {
  if (!coinId || typeof coinId !== "string") {
    throw new CoinGeckoError("coinId is required and must be a non-empty string");
  }

  if (!options.date || !/^\d{2}-\d{2}-\d{4}$/.test(options.date)) {
    throw new CoinGeckoError("date must be in dd-mm-yyyy format");
  }

  const data = await request<unknown>(
    `/coins/${encodeURIComponent(coinId)}/history`,
    {
      date: options.date,
      localization: options.localization ?? false,
    },
  );

  return { success: true, data };
}

/**
 * Fetch trending coins on CoinGecko.
 */
export async function getTrending(): Promise<
  ApiResponse<{ coins: TrendingCoin[] }>
> {
  const data = await request<{ coins: TrendingCoin[] }>("/search/trending");
  return { success: true, data };
}

/**
 * Search for coins, exchanges, and categories.
 */
export async function search(
  query: string,
): Promise<ApiResponse<unknown>> {
  if (!query || typeof query !== "string") {
    throw new CoinGeckoError("query is required and must be a non-empty string");
  }

  const data = await request<unknown>("/search", { query });
  return { success: true, data };
}

/**
 * Fetch global crypto market data.
 */
export async function getGlobal(): Promise<ApiResponse<unknown>> {
  const data = await request<unknown>("/global");
  return { success: true, data };
}

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

export { CoinGeckoError };

export const coingecko = {
  getMarkets,
  getCoinDetail,
  getSimplePrice,
  getMarketChart,
  getCoinHistory,
  getTrending,
  search,
  getGlobal,
} as const;
