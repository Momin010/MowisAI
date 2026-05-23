import {
  useState,
  useEffect,
  useRef,
  useCallback,
  type ChangeEvent,
  type FormEvent,
} from "react";
import { getMarkets, search as searchCoins } from "../api/coingecko";
import type { CoinMarket } from "../types/coingecko";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface HoldingFormData {
  coinId: string;
  coinName: string;
  coinSymbol: string;
  coinImage: string;
  quantity: number;
  purchasePrice: number | null;
}

interface AddHoldingModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSubmit: (data: HoldingFormData) => void;
}

interface SearchResult {
  id: string;
  name: string;
  symbol: string;
  thumb: string;
  market_cap_rank: number | null;
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

const styles = {
  overlay: {
    position: "fixed" as const,
    inset: 0,
    backgroundColor: "rgba(0, 0, 0, 0.4)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    zIndex: 1000,
    padding: "16px",
  },
  modal: {
    backgroundColor: "#fff",
    borderRadius: "8px",
    boxShadow: "0 4px 24px rgba(0, 0, 0, 0.12)",
    width: "100%",
    maxWidth: "480px",
    maxHeight: "90vh",
    overflow: "auto" as const,
  },
  header: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    padding: "20px 24px 0",
  },
  title: {
    fontSize: "1.125rem",
    fontWeight: 600 as const,
    color: "#111",
    margin: 0,
  },
  closeBtn: {
    background: "none",
    border: "none",
    cursor: "pointer",
    padding: "4px",
    color: "#666",
    fontSize: "1.25rem",
    lineHeight: 1,
    borderRadius: "4px",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    width: "32px",
    height: "32px",
  },
  body: {
    padding: "20px 24px 24px",
  },
  field: {
    marginBottom: "20px",
  },
  label: {
    display: "block",
    fontSize: "0.8125rem",
    fontWeight: 500 as const,
    color: "#333",
    marginBottom: "6px",
  },
  input: {
    width: "100%",
    padding: "10px 12px",
    fontSize: "0.9375rem",
    border: "1px solid #e5e5e5",
    borderRadius: "6px",
    outline: "none",
    fontFamily: "inherit",
    color: "#111",
    backgroundColor: "#fff",
    transition: "border-color 0.15s",
    boxSizing: "border-box" as const,
  },
  inputFocus: {
    borderColor: "#0066ff",
  },
  searchResults: {
    border: "1px solid #e5e5e5",
    borderRadius: "6px",
    maxHeight: "200px",
    overflowY: "auto" as const,
    marginTop: "4px",
    backgroundColor: "#fff",
  },
  searchItem: {
    display: "flex",
    alignItems: "center",
    gap: "10px",
    padding: "10px 12px",
    cursor: "pointer",
    borderBottom: "1px solid #f0f0f0",
    transition: "background-color 0.1s",
  },
  searchItemHover: {
    backgroundColor: "#f8f8f8",
  },
  coinImage: {
    width: "24px",
    height: "24px",
    borderRadius: "50%",
    flexShrink: 0,
  },
  coinName: {
    fontSize: "0.875rem",
    fontWeight: 500 as const,
    color: "#111",
  },
  coinSymbol: {
    fontSize: "0.75rem",
    color: "#666",
    marginLeft: "4px",
    textTransform: "uppercase" as const,
  },
  coinRank: {
    fontSize: "0.75rem",
    color: "#999",
    marginLeft: "auto",
  },
  selectedCoin: {
    display: "flex",
    alignItems: "center",
    gap: "10px",
    padding: "10px 12px",
    border: "1px solid #e5e5e5",
    borderRadius: "6px",
    backgroundColor: "#fafafa",
  },
  removeCoinBtn: {
    marginLeft: "auto",
    background: "none",
    border: "none",
    cursor: "pointer",
    color: "#999",
    fontSize: "1.125rem",
    padding: "2px 6px",
    borderRadius: "4px",
  },
  errorText: {
    fontSize: "0.75rem",
    color: "#d32f2f",
    marginTop: "4px",
  },
  loadingText: {
    padding: "12px",
    fontSize: "0.8125rem",
    color: "#666",
    textAlign: "center" as const,
  },
  noResults: {
    padding: "12px",
    fontSize: "0.8125rem",
    color: "#999",
    textAlign: "center" as const,
  },
  row: {
    display: "grid",
    gridTemplateColumns: "1fr 1fr",
    gap: "16px",
  },
  footer: {
    display: "flex",
    justifyContent: "flex-end",
    gap: "12px",
    marginTop: "8px",
  },
  cancelBtn: {
    padding: "10px 20px",
    fontSize: "0.875rem",
    fontWeight: 500 as const,
    border: "1px solid #e5e5e5",
    borderRadius: "6px",
    backgroundColor: "#fff",
    color: "#333",
    cursor: "pointer",
    fontFamily: "inherit",
    transition: "background-color 0.15s",
  },
  submitBtn: {
    padding: "10px 20px",
    fontSize: "0.875rem",
    fontWeight: 500 as const,
    border: "none",
    borderRadius: "6px",
    backgroundColor: "#0066ff",
    color: "#fff",
    cursor: "pointer",
    fontFamily: "inherit",
    transition: "opacity 0.15s",
  },
  submitBtnDisabled: {
    opacity: 0.5,
    cursor: "not-allowed",
  },
} as const;

// ---------------------------------------------------------------------------
// Debounce helper
// ---------------------------------------------------------------------------

function useDebounce<T>(value: T, delay: number): T {
  const [debounced, setDebounced] = useState(value);

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), delay);
    return () => clearTimeout(timer);
  }, [value, delay]);

  return debounced;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const AddHoldingModal = ({ isOpen, onClose, onSubmit }: AddHoldingModalProps) => {
  // Form state
  const [selectedCoin, setSelectedCoin] = useState<CoinMarket | null>(null);
  const [quantity, setQuantity] = useState("");
  const [purchasePrice, setPurchasePrice] = useState("");

  // Search state
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<SearchResult[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);

  // Validation
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [isSubmitting, setIsSubmitting] = useState(false);

  // Refs
  const searchInputRef = useRef<HTMLInputElement>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  const debouncedQuery = useDebounce(searchQuery, 350);

  // Focus search input when modal opens
  useEffect(() => {
    if (isOpen) {
      // Slight delay to allow modal animation/render
      requestAnimationFrame(() => {
        searchInputRef.current?.focus();
      });
    }
  }, [isOpen]);

  // Close on Escape
  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

  // Perform search when debounced query changes
  useEffect(() => {
    if (!debouncedQuery.trim()) {
      setSearchResults([]);
      setSearchError(null);
      return;
    }

    let cancelled = false;

    const performSearch = async () => {
      setIsSearching(true);
      setSearchError(null);

      try {
        const response = await searchCoins(debouncedQuery.trim());

        if (cancelled) return;

        const coins = (response.data as { coins?: SearchResult[] })?.coins ?? [];
        setSearchResults(coins.slice(0, 10));
      } catch {
        if (!cancelled) {
          setSearchError("Failed to search coins. Please try again.");
          setSearchResults([]);
        }
      } finally {
        if (!cancelled) {
          setIsSearching(false);
        }
      }
    };

    performSearch();

    return () => {
      cancelled = true;
    };
  }, [debouncedQuery]);

  // Auto-fill purchase price with current market price when a coin is selected
  useEffect(() => {
    if (selectedCoin?.current_price && !purchasePrice) {
      setPurchasePrice(selectedCoin.current_price.toFixed(2));
    }
  }, [selectedCoin]);

  // Reset form on close
  useEffect(() => {
    if (!isOpen) {
      setSelectedCoin(null);
      setQuantity("");
      setPurchasePrice("");
      setSearchQuery("");
      setSearchResults([]);
      setSearchError(null);
      setErrors({});
      setIsSubmitting(false);
    }
  }, [isOpen]);

  // Handlers
  const handleSelectCoin = useCallback(
    async (result: SearchResult) => {
      // Fetch full market data for the selected coin
      try {
        const response = await getMarkets({ perPage: 250 });
        const coin = response.data.find((c) => c.id === result.id);

        if (coin) {
          setSelectedCoin(coin);
        } else {
          // Fallback: create minimal market data from search result
          setSelectedCoin({
            id: result.id,
            symbol: result.symbol,
            name: result.name,
            image: result.thumb,
            current_price: 0,
            market_cap: 0,
            market_cap_rank: result.market_cap_rank,
            fully_diluted_valuation: null,
            total_volume: 0,
            high_24h: 0,
            low_24h: 0,
            price_change_24h: 0,
            price_change_percentage_24h: 0,
            market_cap_change_24h: 0,
            market_cap_change_percentage_24h: 0,
            circulating_supply: 0,
            total_supply: null,
            max_supply: null,
            ath: 0,
            ath_change_percentage: 0,
            ath_date: "",
            atl: 0,
            atl_change_percentage: 0,
            atl_date: "",
            roi: null,
            last_updated: "",
          });
        }
      } catch {
        // If market fetch fails, still select the coin with basic info
        setSelectedCoin({
          id: result.id,
          symbol: result.symbol,
          name: result.name,
          image: result.thumb,
          current_price: 0,
          market_cap: 0,
          market_cap_rank: result.market_cap_rank,
          fully_diluted_valuation: null,
          total_volume: 0,
          high_24h: 0,
          low_24h: 0,
          price_change_24h: 0,
          price_change_percentage_24h: 0,
          market_cap_change_24h: 0,
          market_cap_change_percentage_24h: 0,
          circulating_supply: 0,
          total_supply: null,
          max_supply: null,
          ath: 0,
          ath_change_percentage: 0,
          ath_date: "",
          atl: 0,
          atl_change_percentage: 0,
          atl_date: "",
          roi: null,
          last_updated: "",
        });
      }

      setSearchQuery("");
      setSearchResults([]);
    },
    [],
  );

  const handleRemoveCoin = useCallback(() => {
    setSelectedCoin(null);
    setPurchasePrice("");
    setQuantity("");
    searchInputRef.current?.focus();
  }, []);

  const validate = useCallback((): boolean => {
    const newErrors: Record<string, string> = {};

    if (!selectedCoin) {
      newErrors.coin = "Please select a cryptocurrency.";
    }

    const qty = parseFloat(quantity);
    if (!quantity.trim()) {
      newErrors.quantity = "Quantity is required.";
    } else if (isNaN(qty) || qty <= 0) {
      newErrors.quantity = "Quantity must be a positive number.";
    }

    const price = parseFloat(purchasePrice);
    if (purchasePrice.trim() && (isNaN(price) || price < 0)) {
      newErrors.purchasePrice = "Price must be a non-negative number.";
    }

    setErrors(newErrors);
    return Object.keys(newErrors).length === 0;
  }, [selectedCoin, quantity, purchasePrice]);

  const handleSubmit = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();

      if (!validate() || !selectedCoin) return;

      setIsSubmitting(true);

      try {
        onSubmit({
          coinId: selectedCoin.id,
          coinName: selectedCoin.name,
          coinSymbol: selectedCoin.symbol,
          coinImage: selectedCoin.image,
          quantity: parseFloat(quantity),
          purchasePrice: purchasePrice.trim() ? parseFloat(purchasePrice) : null,
        });
      } finally {
        setIsSubmitting(false);
      }
    },
    [selectedCoin, quantity, purchasePrice, onSubmit, validate],
  );

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === overlayRef.current) {
        onClose();
      }
    },
    [onClose],
  );

  // Don't render anything if not open
  if (!isOpen) return null;

  return (
    <div
      ref={overlayRef}
      style={styles.overlay}
      onClick={handleOverlayClick}
      role="dialog"
      aria-modal="true"
      aria-label="Add holding"
    >
      <div style={styles.modal}>
        {/* Header */}
        <div style={styles.header}>
          <h2 style={styles.title}>Add Holding</h2>
          <button
            type="button"
            style={styles.closeBtn}
            onClick={onClose}
            aria-label="Close modal"
          >
            &#x2715;
          </button>
        </div>

        {/* Body */}
        <form onSubmit={handleSubmit} style={styles.body}>
          {/* Coin Selection */}
          <div style={styles.field}>
            <label style={styles.label}>Cryptocurrency</label>

            {selectedCoin ? (
              <div style={styles.selectedCoin}>
                <img
                  src={selectedCoin.image}
                  alt={selectedCoin.name}
                  style={styles.coinImage}
                  loading="lazy"
                />
                <span style={styles.coinName}>{selectedCoin.name}</span>
                <span style={styles.coinSymbol}>{selectedCoin.symbol}</span>
                {selectedCoin.current_price > 0 && (
                  <span style={styles.coinRank}>
                    ${selectedCoin.current_price.toLocaleString()}
                  </span>
                )}
                <button
                  type="button"
                  style={styles.removeCoinBtn}
                  onClick={handleRemoveCoin}
                  aria-label="Remove selected coin"
                >
                  &#x2715;
                </button>
              </div>
            ) : (
              <>
                <input
                  ref={searchInputRef}
                  type="text"
                  placeholder="Search for a coin..."
                  value={searchQuery}
                  onChange={(e: ChangeEvent<HTMLInputElement>) =>
                    setSearchQuery(e.target.value)
                  }
                  style={styles.input}
                  autoComplete="off"
                  aria-label="Search for a cryptocurrency"
                />

                {isSearching && (
                  <div style={styles.loadingText}>Searching...</div>
                )}

                {searchError && (
                  <div style={styles.errorText}>{searchError}</div>
                )}

                {!isSearching && searchResults.length > 0 && (
                  <div style={styles.searchResults} role="listbox">
                    {searchResults.map((coin) => (
                      <div
                        key={coin.id}
                        style={styles.searchItem}
                        role="option"
                        aria-selected={false}
                        tabIndex={0}
                        onClick={() => handleSelectCoin(coin)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            handleSelectCoin(coin);
                          }
                        }}
                      >
                        <img
                          src={coin.thumb}
                          alt={coin.name}
                          style={styles.coinImage}
                          loading="lazy"
                        />
                        <span style={styles.coinName}>{coin.name}</span>
                        <span style={styles.coinSymbol}>{coin.symbol}</span>
                        {coin.market_cap_rank != null && (
                          <span style={styles.coinRank}>
                            #{coin.market_cap_rank}
                          </span>
                        )}
                      </div>
                    ))}
                  </div>
                )}

                {!isSearching &&
                  !searchError &&
                  debouncedQuery.trim() &&
                  searchResults.length === 0 && (
                    <div style={styles.noResults}>No coins found.</div>
                  )}
              </>
            )}

            {errors.coin && <div style={styles.errorText}>{errors.coin}</div>}
          </div>

          {/* Quantity & Purchase Price */}
          <div style={styles.row}>
            <div style={styles.field}>
              <label style={styles.label} htmlFor="holding-quantity">
                Quantity
              </label>
              <input
                id="holding-quantity"
                type="number"
                placeholder="0.00"
                value={quantity}
                onChange={(e: ChangeEvent<HTMLInputElement>) =>
                  setQuantity(e.target.value)
                }
                style={styles.input}
                min="0"
                step="any"
                aria-invalid={!!errors.quantity}
                aria-describedby={
                  errors.quantity ? "quantity-error" : undefined
                }
              />
              {errors.quantity && (
                <div id="quantity-error" style={styles.errorText}>
                  {errors.quantity}
                </div>
              )}
            </div>

            <div style={styles.field}>
              <label style={styles.label} htmlFor="holding-price">
                Purchase Price (USD)
              </label>
              <input
                id="holding-price"
                type="number"
                placeholder="0.00"
                value={purchasePrice}
                onChange={(e: ChangeEvent<HTMLInputElement>) =>
                  setPurchasePrice(e.target.value)
                }
                style={styles.input}
                min="0"
                step="any"
                aria-invalid={!!errors.purchasePrice}
                aria-describedby={
                  errors.purchasePrice ? "price-error" : undefined
                }
              />
              {errors.purchasePrice && (
                <div id="price-error" style={styles.errorText}>
                  {errors.purchasePrice}
                </div>
              )}
            </div>
          </div>

          {/* Footer */}
          <div style={styles.footer}>
            <button type="button" style={styles.cancelBtn} onClick={onClose}>
              Cancel
            </button>
            <button
              type="submit"
              style={{
                ...styles.submitBtn,
                ...(isSubmitting ? styles.submitBtnDisabled : {}),
              }}
              disabled={isSubmitting}
            >
              {isSubmitting ? "Adding..." : "Add Holding"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
};

export default AddHoldingModal;
