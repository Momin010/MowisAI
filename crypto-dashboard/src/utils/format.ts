/**
 * Formatting utilities for common data transformations.
 * All functions handle edge cases and invalid input gracefully.
 */

/**
 * Format a number as currency string.
 * @param amount - The amount to format
 * @param currency - ISO 4217 currency code (default: 'USD')
 * @param locale - BCP 47 locale string (default: 'en-US')
 * @returns Formatted currency string
 */
export function formatCurrency(
  amount: number,
  currency: string = 'USD',
  locale: string = 'en-US'
): string {
  if (typeof amount !== 'number' || !isFinite(amount)) {
    return '$0.00';
  }

  try {
    return new Intl.NumberFormat(locale, {
      style: 'currency',
      currency,
    }).format(amount);
  } catch {
    return `${currency} ${amount.toFixed(2)}`;
  }
}

/**
 * Format a number with thousand separators and decimal places.
 * @param value - The number to format
 * @param decimals - Number of decimal places (default: 0)
 * @param locale - BCP 47 locale string (default: 'en-US')
 * @returns Formatted number string
 */
export function formatNumber(
  value: number,
  decimals: number = 0,
  locale: string = 'en-US'
): string {
  if (typeof value !== 'number' || !isFinite(value)) {
    return '0';
  }

  try {
    return new Intl.NumberFormat(locale, {
      minimumFractionDigits: decimals,
      maximumFractionDigits: decimals,
    }).format(value);
  } catch {
    return value.toFixed(decimals);
  }
}

/**
 * Format a number as a compact representation (e.g., 1K, 2.5M).
 * @param value - The number to format
 * @param locale - BCP 47 locale string (default: 'en-US')
 * @returns Compact formatted string
 */
export function formatCompactNumber(
  value: number,
  locale: string = 'en-US'
): string {
  if (typeof value !== 'number' || !isFinite(value)) {
    return '0';
  }

  try {
    return new Intl.NumberFormat(locale, {
      notation: 'compact',
      compactDisplay: 'short',
      maximumFractionDigits: 1,
    }).format(value);
  } catch {
    // Fallback for environments without Intl support
    const absValue = Math.abs(value);
    const sign = value < 0 ? '-' : '';

    if (absValue >= 1_000_000_000) {
      return `${sign}${(absValue / 1_000_000_000).toFixed(1)}B`;
    }
    if (absValue >= 1_000_000) {
      return `${sign}${(absValue / 1_000_000).toFixed(1)}M`;
    }
    if (absValue >= 1_000) {
      return `${sign}${(absValue / 1_000).toFixed(1)}K`;
    }
    return `${sign}${absValue}`;
  }
}

/**
 * Format a number as a percentage string.
 * @param value - The value to format (0.5 = 50%)
 * @param decimals - Number of decimal places (default: 0)
 * @param locale - BCP 47 locale string (default: 'en-US')
 * @returns Formatted percentage string
 */
export function formatPercentage(
  value: number,
  decimals: number = 0,
  locale: string = 'en-US'
): string {
  if (typeof value !== 'number' || !isFinite(value)) {
    return '0%';
  }

  try {
    return new Intl.NumberFormat(locale, {
      style: 'percent',
      minimumFractionDigits: decimals,
      maximumFractionDigits: decimals,
    }).format(value);
  } catch {
    return `${(value * 100).toFixed(decimals)}%`;
  }
}

/**
 * Format a date to a localized string.
 * @param date - Date object, ISO string, or timestamp
 * @param style - Predefined format style ('short', 'medium', 'long', 'full')
 * @param locale - BCP 47 locale string (default: 'en-US')
 * @returns Formatted date string
 */
export function formatDate(
  date: Date | string | number,
  style: 'short' | 'medium' | 'long' | 'full' = 'medium',
  locale: string = 'en-US'
): string {
  const parsed = parseDate(date);
  if (!parsed) {
    return 'Invalid date';
  }

  const dateStyleOptions: Record<string, Intl.DateTimeFormatOptions> = {
    short: { year: 'numeric', month: 'numeric', day: 'numeric' },
    medium: { year: 'numeric', month: 'short', day: 'numeric' },
    long: { year: 'numeric', month: 'long', day: 'numeric' },
    full: { year: 'numeric', month: 'long', day: 'numeric', weekday: 'long' },
  };

  try {
    return new Intl.DateTimeFormat(locale, dateStyleOptions[style]).format(parsed);
  } catch {
    return parsed.toISOString().split('T')[0];
  }
}

/**
 * Format a date to a localized time string.
 * @param date - Date object, ISO string, or timestamp
 * @param includeSeconds - Whether to include seconds (default: false)
 * @param locale - BCP 47 locale string (default: 'en-US')
 * @returns Formatted time string
 */
export function formatTime(
  date: Date | string | number,
  includeSeconds: boolean = false,
  locale: string = 'en-US'
): string {
  const parsed = parseDate(date);
  if (!parsed) {
    return 'Invalid time';
  }

  try {
    return new Intl.DateTimeFormat(locale, {
      hour: 'numeric',
      minute: '2-digit',
      ...(includeSeconds && { second: '2-digit' }),
    }).format(parsed);
  } catch {
    const hours = parsed.getHours();
    const minutes = parsed.getMinutes().toString().padStart(2, '0');
    const period = hours >= 12 ? 'PM' : 'AM';
    const displayHours = hours % 12 || 12;
    const seconds = includeSeconds
      ? `:${parsed.getSeconds().toString().padStart(2, '0')}`
      : '';
    return `${displayHours}:${minutes}${seconds} ${period}`;
  }
}

/**
 * Format a date to a relative time string (e.g., "2 hours ago", "in 3 days").
 * @param date - Date object, ISO string, or timestamp
 * @param baseDate - The base date to compare against (default: now)
 * @returns Relative time string
 */
export function formatRelativeTime(
  date: Date | string | number,
  baseDate: Date | string | number = new Date()
): string {
  const target = parseDate(date);
  const base = parseDate(baseDate);

  if (!target || !base) {
    return 'Invalid date';
  }

  const diffMs = target.getTime() - base.getTime();
  const absDiff = Math.abs(diffMs);
  const isFuture = diffMs > 0;

  const units: Array<{ unit: Intl.RelativeTimeFormatUnit; ms: number }> = [
    { unit: 'year', ms: 365 * 24 * 60 * 60 * 1000 },
    { unit: 'month', ms: 30 * 24 * 60 * 60 * 1000 },
    { unit: 'week', ms: 7 * 24 * 60 * 60 * 1000 },
    { unit: 'day', ms: 24 * 60 * 60 * 1000 },
    { unit: 'hour', ms: 60 * 60 * 1000 },
    { unit: 'minute', ms: 60 * 1000 },
    { unit: 'second', ms: 1000 },
  ];

  for (const { unit, ms } of units) {
    const value = Math.floor(absDiff / ms);
    if (value >= 1) {
      try {
        const rtf = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });
        return rtf.format(isFuture ? value : -value, unit);
      } catch {
        // Fallback
        const suffix = isFuture ? 'from now' : 'ago';
        return `${value} ${unit}${value > 1 ? 's' : ''} ${suffix}`;
      }
    }
  }

  return 'just now';
}

/**
 * Format a duration in milliseconds to a human-readable string.
 * @param ms - Duration in milliseconds
 * @param includeMs - Whether to include milliseconds (default: false)
 * @returns Formatted duration string (e.g., "2h 30m 15s")
 */
export function formatDuration(ms: number, includeMs: boolean = false): string {
  if (typeof ms !== 'number' || !isFinite(ms) || ms < 0) {
    return '0s';
  }

  const totalSeconds = Math.floor(ms / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const milliseconds = ms % 1000;

  const parts: string[] = [];

  if (hours > 0) parts.push(`${hours}h`);
  if (minutes > 0) parts.push(`${minutes}m`);
  if (seconds > 0 || parts.length === 0) parts.push(`${seconds}s`);
  if (includeMs && milliseconds > 0) parts.push(`${milliseconds}ms`);

  return parts.join(' ');
}

/**
 * Format a file size in bytes to a human-readable string.
 * @param bytes - File size in bytes
 * @param decimals - Number of decimal places (default: 1)
 * @returns Formatted file size string (e.g., "1.5 MB")
 */
export function formatFileSize(bytes: number, decimals: number = 1): string {
  if (typeof bytes !== 'number' || !isFinite(bytes) || bytes < 0) {
    return '0 B';
  }

  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  const k = 1024;

  if (bytes === 0) return '0 B';

  const i = Math.floor(Math.log(bytes) / Math.log(k));
  const unitIndex = Math.min(i, units.length - 1);
  const value = bytes / Math.pow(k, unitIndex);

  return `${value.toFixed(unitIndex === 0 ? 0 : decimals)} ${units[unitIndex]}`;
}

/**
 * Truncate text to a specified length with ellipsis.
 * @param text - The text to truncate
 * @param maxLength - Maximum length including the suffix
 * @param suffix - The suffix to append (default: '...')
 * @returns Truncated string
 */
export function truncate(text: string, maxLength: number, suffix: string = '...'): string {
  if (typeof text !== 'string') {
    return '';
  }

  if (text.length <= maxLength) {
    return text;
  }

  const truncatedLength = maxLength - suffix.length;

  if (truncatedLength <= 0) {
    return suffix.slice(0, maxLength);
  }

  return text.slice(0, truncatedLength).trimEnd() + suffix;
}

/**
 * Format a phone number string.
 * @param phone - Raw phone number string (digits only or with formatting)
 * @param format - Output format (default: 'national')
 * @returns Formatted phone number
 */
export function formatPhoneNumber(
  phone: string,
  format: 'national' | 'international' | 'e164' = 'national'
): string {
  if (typeof phone !== 'string') {
    return '';
  }

  const digits = phone.replace(/\D/g, '');

  if (digits.length < 10) {
    return phone;
  }

  // Handle US numbers (10 or 11 digits)
  const nationalNumber = digits.length === 11 && digits.startsWith('1')
    ? digits.slice(1)
    : digits.slice(-10);

  const areaCode = nationalNumber.slice(0, 3);
  const exchange = nationalNumber.slice(3, 6);
  const subscriber = nationalNumber.slice(6);

  switch (format) {
    case 'international':
      return `+1 (${areaCode}) ${exchange}-${subscriber}`;
    case 'e164':
      return `+1${nationalNumber}`;
    case 'national':
    default:
      return `(${areaCode}) ${exchange}-${subscriber}`;
  }
}

/**
 * Convert a string to title case.
 * @param text - The input string
 * @returns Title cased string
 */
export function toTitleCase(text: string): string {
  if (typeof text !== 'string') {
    return '';
  }

  const minorWords = new Set([
    'a', 'an', 'the', 'and', 'but', 'or', 'nor', 'for', 'yet', 'so',
    'in', 'on', 'at', 'to', 'by', 'of', 'with', 'from', 'as', 'into',
  ]);

  return text
    .toLowerCase()
    .split(/\s+/)
    .map((word, index, words) => {
      if (word.length === 0) return word;

      // Always capitalize first and last word
      if (index === 0 || index === words.length - 1) {
        return capitalizeFirst(word);
      }

      // Don't capitalize minor words
      if (minorWords.has(word)) {
        return word;
      }

      return capitalizeFirst(word);
    })
    .join(' ');
}

/**
 * Convert a string to a URL-friendly slug.
 * @param text - The input string
 * @returns URL-safe slug
 */
export function toSlug(text: string): string {
  if (typeof text !== 'string') {
    return '';
  }

  return text
    .toLowerCase()
    .trim()
    .replace(/[^\w\s-]/g, '')       // Remove non-word chars
    .replace(/[\s_]+/g, '-')        // Replace spaces and underscores with hyphens
    .replace(/-+/g, '-')            // Collapse multiple hyphens
    .replace(/^-+|-+$/g, '');       // Trim leading/trailing hyphens
}

/**
 * Mask sensitive data in a string (e.g., email, SSN).
 * @param text - The text to mask
 * @param visibleStart - Number of visible characters at start (default: 2)
 * @param visibleEnd - Number of visible characters at end (default: 2)
 * @param maskChar - Character to use for masking (default: '*')
 * @returns Masked string
 */
export function maskText(
  text: string,
  visibleStart: number = 2,
  visibleEnd: number = 2,
  maskChar: string = '*'
): string {
  if (typeof text !== 'string' || text.length === 0) {
    return '';
  }

  const totalVisible = visibleStart + visibleEnd;

  if (text.length <= totalVisible) {
    return maskChar.repeat(text.length);
  }

  const start = text.slice(0, visibleStart);
  const end = text.slice(-visibleEnd);
  const maskedLength = text.length - totalVisible;

  return `${start}${maskChar.repeat(maskedLength)}${end}`;
}

/**
 * Format a list of items into a human-readable string with conjunction.
 * @param items - Array of strings to format
 * @param conjunction - The conjunction to use (default: 'and')
 * @param locale - BCP 47 locale string (default: 'en-US')
 * @returns Formatted list string (e.g., "A, B, and C")
 */
export function formatList(
  items: string[],
  conjunction: string = 'and',
  locale: string = 'en-US'
): string {
  if (!Array.isArray(items) || items.length === 0) {
    return '';
  }

  try {
    // Use Intl.ListFormat if available
    const ListFormat = (Intl as any).ListFormat;
    if (ListFormat) {
      return new ListFormat(locale, {
        style: 'long',
        type: 'conjunction',
      }).format(items);
    }
    throw new Error('ListFormat not available');
  } catch {
    // Fallback
    if (items.length === 1) return items[0];
    if (items.length === 2) return `${items[0]} ${conjunction} ${items[1]}`;

    const allButLast = items.slice(0, -1).join(', ');
    const last = items[items.length - 1];
    return `${allButLast}, ${conjunction} ${last}`;
  }
}

// ── Internal helpers ────────────────────────────────────────────────────────

function parseDate(date: Date | string | number): Date | null {
  if (date instanceof Date) {
    return isNaN(date.getTime()) ? null : date;
  }

  if (typeof date === 'number') {
    const d = new Date(date);
    return isNaN(d.getTime()) ? null : d;
  }

  if (typeof date === 'string') {
    const d = new Date(date);
    return isNaN(d.getTime()) ? null : d;
  }

  return null;
}

function capitalizeFirst(str: string): string {
  if (str.length === 0) return str;
  return str.charAt(0).toUpperCase() + str.slice(1);
}
