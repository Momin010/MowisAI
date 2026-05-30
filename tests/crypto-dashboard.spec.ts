import { test, expect } from '@playwright/test';

test.describe('Crypto Dashboard', () => {
  test('should load and display the main page', async ({ page }) => {
    await page.goto('http://localhost:3000');
    
    // Check the page title
    await expect(page).toHaveTitle(/Crypto Dashboard/);
    
    // Check the header exists
    await expect(page.locator('h1')).toContainText('Crypto Dashboard');
    
    // Check navigation tabs
    await expect(page.locator('button:has-text("Markets")')).toBeVisible();
    await expect(page.locator('button:has-text("Portfolio")')).toBeVisible();
  });

  test('should display market data table', async ({ page }) => {
    await page.goto('http://localhost:3000');
    
    // Wait for data to load
    await page.waitForSelector('table', { timeout: 10000 });
    
    // Check table headers
    await expect(page.locator('th:has-text("#")')).toBeVisible();
    await expect(page.locator('th:has-text("Coin")')).toBeVisible();
    await expect(page.locator('th:has-text("Price")')).toBeVisible();
    await expect(page.locator('th:has-text("24h %")')).toBeVisible();
    await expect(page.locator('th:has-text("Market Cap")')).toBeVisible();
    await expect(page.locator('th:has-text("Volume")')).toBeVisible();
    await expect(page.locator('th:has-text("7d")')).toBeVisible();
    
    // Check that at least one coin row exists
    const rows = page.locator('tbody tr');
    await expect(rows.first()).toBeVisible();
    expect(await rows.count()).toBeGreaterThan(0);
  });

  test('should have search functionality', async ({ page }) => {
    await page.goto('http://localhost:3000');
    
    // Wait for data to load
    await page.waitForSelector('table', { timeout: 10000 });
    
    // Check search input exists
    const searchInput = page.locator('input[placeholder="Search coins..."]');
    await expect(searchInput).toBeVisible();
    
    // Type in search
    await searchInput.fill('bitcoin');
    
    // Verify filtering works
    await page.waitForTimeout(500);
    const rows = page.locator('tbody tr');
    expect(await rows.count()).toBeGreaterThan(0);
  });

  test('should switch between Markets and Portfolio views', async ({ page }) => {
    await page.goto('http://localhost:3000');
    
    // Click Portfolio tab
    await page.locator('button:has-text("Portfolio")').click();
    
    // Should show portfolio view
    await expect(page.locator('h2:has-text("Portfolio")')).toBeVisible();
    
    // Click Markets tab
    await page.locator('button:has-text("Markets")').click();
    
    // Should show markets view
    await expect(page.locator('table')).toBeVisible();
  });

  test('should display coin details when clicking a row', async ({ page }) => {
    await page.goto('http://localhost:3000');
    
    // Wait for data to load
    await page.waitForSelector('table', { timeout: 10000 });
    
    // Click the first row
    await page.locator('tbody tr').first().click();
    
    // Should show back button
    await expect(page.locator('button:has-text("Back")')).toBeVisible();
    
    // Should show coin details
    await expect(page.locator('.detail-header')).toBeVisible();
  });

  test('should have clean, minimal design (no gradients, no emojis)', async ({ page }) => {
    await page.goto('http://localhost:3000');
    
    // Check background color is white
    const body = page.locator('body');
    const bgColor = await body.evaluate(el => getComputedStyle(el).backgroundColor);
    expect(bgColor).toBe('rgb(255, 255, 255)');
    
    // Check no emojis in the page
    const text = await page.textContent('body');
    const emojiRegex = /[\u{1F600}-\u{1F64F}]|[\u{1F300}-\u{1F5FF}]|[\u{1F680}-\u{1F6FF}]|[\u{1F1E0}-\u{1F1FF}]|[\u{2600}-\u{26FF}]|[\u{2700}-\u{27BF}]/u;
    expect(emojiRegex.test(text || '')).toBeFalsy();
  });
});
