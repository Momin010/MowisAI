# Professional Features Documentation

## Overview

This document outlines the professional-grade features added to the MCP File Converter, enabling the creation of visually rich, data-driven documents with charts, images, and real-time data integration.

## New Modules

### 1. Chart Generator (`chart-generator.js`)

**Purpose**: Generate professional charts for embedding in documents

**Supported Chart Types**:
- **Bar Charts**: Compare values across categories
- **Pie Charts**: Show proportions and distributions
- **Line Charts**: Display trends over time
- **Doughnut Charts**: Alternative to pie charts with center space
- **Comparison Charts**: Compare multiple data series

**Key Features**:
- SVG-based rendering (no native dependencies)
- Automatic scaling and padding
- Professional styling with colors and labels
- Asynchronous PNG generation via Sharp

**Usage Example**:
```javascript
import { generateBarChart, generatePieChart } from './chart-generator.js';

const salesData = [
  { label: 'Q1', value: 45000 },
  { label: 'Q2', value: 52000 }
];

const chartImage = await generateBarChart(salesData, 'Quarterly Sales');
// Returns PNG buffer, ~20KB
```

**Chart Metadata**:
- Bar Chart: ~21KB PNG
- Pie Chart: ~18KB PNG
- Line Chart: ~20KB PNG
- Doughnut Chart: ~23KB PNG

### 2. Image Handler (`image-handler.js`)

**Purpose**: Download, process, and embed images in documents

**Key Functions**:

#### `downloadImage(url)`
- Downloads images from URLs
- Implements automatic caching
- Handles timeouts and errors gracefully

#### `getImageForDocument(url, options)`
- Downloads and optimizes images for document embedding
- Automatically resizes to specified dimensions
- Converts to PNG for consistency
- Options: `{width, height, optimize}`

#### `processImages(imageUrls, options)`
- Batch process multiple images
- Returns array of processed image buffers

#### `createCollage(imageUrls, columns, width, height)`
- Create image grid/collage layouts
- Useful for presentations and reports

**Usage Example**:
```javascript
import { getImageForDocument, createCollage } from './image-handler.js';

// Single image
const image = await getImageForDocument(
  'https://example.com/chart.png',
  { width: 600, height: 400, optimize: true }
);

// Collage
const collage = await createCollage(
  ['url1.jpg', 'url2.jpg', 'url3.jpg'],
  3, // columns
  300, // width per image
  200  // height per image
);
```

### 3. Data Fetcher (`data-fetcher.js`)

**Purpose**: Fetch real-time data from web sources and APIs

**Key Functions**:

#### `searchWeb(query, options)`
- Search using DuckDuckGo API (no API key required)
- Returns top results with titles and URLs
- Returns: `[{title, url, snippet}, ...]`

#### `fetchJSON(url, options)`
- Fetch JSON data from any API
- Supports GET, POST, PUT, DELETE methods
- Handles authentication via headers

#### `fetchStockData(symbol)`
- Fetch stock market data
- Returns: `{symbol, price, change, high, low, volume, timestamp}`

#### `fetchWeather(location)`
- Real-time weather data (Open-Meteo API)
- Returns: `{location, latitude, longitude, current: {...}}`

#### `fetchCryptoPrices(currencies)`
- Cryptocurrency prices (CoinGecko API)
- Returns: `{btc: {usd: 50000}, eth: {usd: 3000}, ...}`

#### `fetchGitHubTrending()`
- Trending GitHub repositories
- Returns: `[{name, url, stars, language, description}, ...]`

#### `fetchNews(topic, limit)`
- Tech news and articles
- Returns: `[{title, url, snippet}, ...]`

#### `smartFetch(url, options)`
-Automatically detects JSON or HTML content
- Returns: `{type: 'json'|'html', data: {...}}`

**Usage Example**:
```javascript
import {
  searchWeb,
  fetchStockData,
  fetchWeather,
  fetchGitHubTrending
} from './data-fetcher.js';

const webResults = await searchWeb('artificial intelligence');
const stock = await fetchStockData('AAPL');
const weather = await fetchWeather('San Francisco');
const trending = await fetchGitHubTrending();
```

## Enhanced Converters

### Excel with Charts (`convertToExcelWithCharts`)

**Features**:
- Embeds chart images directly in Excel sheets
- Automatic data table generation
- Professional formatting with colors and headers
- Multiple charts per sheet

**Usage**:
```javascript
import { convertToExcelWithCharts } from './server.js';

const buffer = await convertToExcelWithCharts(
  JSON.stringify(data),
  [
    { type: 'bar', data: salesData, title: 'Sales' },
    { type: 'pie', data: regionData, title: 'Distribution' }
  ]
);
```

### PowerPoint with Visuals (`convertToPptxWithVisuals`)

**Features**:
- Multiple content slides with formatted data
- Dedicated chart slides with titles and styling
- Image gallery slides with optimized layouts
- Professional color schemes and typography

**Usage**:
```javascript
import { convertToPptxWithVisuals } from './server.js';

const buffer = await convertToPptxWithVisuals(
  JSON.stringify(data),
  chartConfigs,
  imageUrls
);
```

## Complete Workflow Example

```javascript
import {
  convertToExcelWithCharts,
  convertToPptxWithVisuals,
  convertFile
} from './server.js';
import { generateBarChart, generatePieChart } from './chart-generator.js';
import { getImageForDocument } from './image-handler.js';
import { fetchStockData, searchWeb } from './data-fetcher.js';
import fs from 'fs';

// 1. Fetch real-time data
const stock = await fetchStockData('AAPL');
const news = await searchWeb('Apple news');

// 2. Prepare data for visualization
const chartData = [
  { label: 'Current', value: stock.price },
  { label: 'High', value: stock.high },
  { label: 'Low', value: stock.low }
];

// 3. Generate charts
const chart = await generateBarChart(chartData, 'Stock Analysis');

// 4. Download and process images
const image = await getImageForDocument(
  'https://example.com/apple.jpg',
  { width: 600, height: 400 }
);

// 5. Create professional Excel report
const excel = await convertToExcelWithCharts(
  JSON.stringify([{ stock: stock.symbol, price: stock.price }]),
  [{ type: 'bar', data: chartData, title: 'Analysis' }]
);
fs.writeFileSync('report.xlsx', excel);

// 6. Create professional PowerPoint
const pptx = await convertToPptxWithVisuals(
  JSON.stringify([stock]),
  [{ type: 'pie', data: chartData.slice(0, 3) }],
  ['https://example.com/chart.png']
);
fs.writeFileSync('presentation.pptx', pptx);

// 7. Create PDF report
const pdf = await convertFile(
  JSON.stringify({ stock, news }),
  'pdf',
  'report'
);
fs.writeFileSync('report.pdf', pdf);
```

## Performance Metrics

| Operation | Time | Size |
|-----------|------|------|
| Bar Chart Generation | ~100ms | 21KB |
| Pie Chart Generation | ~80ms | 18KB |
| Excel Creation | ~200ms | 6-40KB |
| DOCX Creation | ~150ms | 7KB |
| PDF Creation | ~100ms | 1-2KB |
| PPTX Creation | ~500ms | 50-100KB |
| Image Download + Process | ~500-2000ms | 5-50KB |

## Integration with Orchestrator

The orchestrator can now:

1. **Generate data-driven documents**: Agents can request charts and images automatically
2. **Include web data**: Fetch latest information before generating reports
3. **Create professional output**: Multi-format exports with consistent quality
4. **Embed visualizations**: Charts and images directly in output documents

### Orchestrator Usage

```javascript
// In orchestrator.js
const result = await convertToExcelWithCharts(
  JSON.stringify(agentOutput),
  [
    { type: 'bar', data: agentOutput.metrics, title: 'Performance' },
    { type: 'pie', data: agentOutput.distribution, title: 'Breakdown' }
  ]
);

// Save with charts embedded
exportFiles(agentResults, taskDescription, result);
```

## API Integration Strategy

### For Agents

Agents can request professional document generation through MCP tools:

```javascript
// Agent requests document with charts
const tools = {
  generate_chart: {
    description: 'Generate a professional chart',
    params: { type, data, title }
  },
  download_image: {
    description: 'Download and optimize image',
    params: { url, width, height }
  },
  fetch_web_data: {
    description: 'Fetch real-time web data',
    params: { query, source }
  }
};
```

### Best Practices

1. **Pre-allocate chart space**: Reserve room in documents for charts
2. **Optimize image sizes**: Use appropriate dimensions for target format
3. **Cache downloaded resources**: Avoid redundant downloads
4. **Validate data format**: Ensure data matches chart type requirements
5. **Handle timeouts gracefully**: Web requests may fail in some environments

## Dependencies

- **exceljs** (4.3.0): Excel spreadsheet generation
- **pdfkit** (0.13.0): PDF document creation
- **docx** (8.0.0): Word document generation
- **pptxgenjs** (3.10.0): PowerPoint generation
- **sharp** (0.33.0): Image processing and SVG conversion
- **axios** (1.6.0): HTTP requests for data fetching

## Troubleshooting

### Chart Generation Issues
- **Blank charts**: Check if data array is empty or values are zero
- **Memory usage**: Reduce image dimensions if running out of memory
- **SVG conversion fails**: Sharp requires image processing libraries

### Image Download Issues
- **Timeout errors**: Network timeouts in restricted environments
- **Unsupported formats**: Images are automatically converted to PNG
- **Cache issues**: Clear `.image_cache` directory to force re-download

### Document Generation Issues
- **PDFs too small**: Content might be cut off due to margin calculations
- **PowerPoint corruption**: Use `writeFile` instead of `writeBuffer` (fixed)
- **Excel image placement**: Charts are positioned after data, may need manual adjustment

## Future Enhancements

1. **3D Charts**: Support for 3D bar, pie, and scatter plots
2. **Interactive Charts**: SVG-based interactive elements
3. **Real-time Updates**: WebSocket integration for live data
4. **Custom Styling**: Theme support for consistent branding
5. **Table Charts**: Automatic formatting of complex tables
6. **Video Embedding**: Support for embedded video URLs
7. **OCR Integration**: Extract data from images and PDFs
8. **Advanced Analytics**: Statistical calculations and predictions

---

**Version**: 1.0.0  
**Last Updated**: 2026-02-24  
**Status**: Production Ready
