# MCP File Converter - Agent Guide

This guide explains how agents should use the MCP File Converter to generate properly formatted files.

## Overview

The MCP File Converter allows agents to convert structured data (JSON/text) into professional binary formats. Instead of generating corrupted plain-text files, agents should use the converter to create:

- ✅ **Excel** (.xlsx) - Financial models, data tables
- ✅ **PDF** - Reports, documents
- ✅ **Word** (.docx) - Formatted documents with tables
- ✅ **PowerPoint** (.pptx) - Presentations
- ✅ **CSV** - Data exports

## For Financial Analyst Agent

When creating financial models, use the converter:

```javascript
// Instead of writing plain text, prepare structured data:
const financialModel = {
  title: "5-Year Financial Forecast",
  data: [
    { Year: 2024, Revenue: 1000000, Expenses: 300000, Profit: 700000 },
    { Year: 2025, Revenue: 1500000, Expenses: 450000, Profit: 1050000 },
    { Year: 2026, Revenue: 2250000, Expenses: 675000, Profit: 1575000 }
  ]
};

// Call the converter via file system (orchestrator will handle MCP):
// Output file name: forecast.xlsx will trigger Excel conversion
const content = JSON.stringify(financialModel);
// Write to orchestrator as JSON, orchestrator converts to .xlsx
```

## For Writer/Researcher Agents

When creating reports, use the converter:

```javascript
// Prepare structured data:
const report = {
  title: "Market Analysis Report",
  sections: [
    { heading: "Executive Summary", content: "..." },
    { heading: "Market Overview", content: "..." },
    { heading: "Recommendations", content: "..." }
  ]
};

const content = JSON.stringify(report);
// Output file name: report.docx will trigger Word conversion
```

## For Data Scientist Agent

When creating presentations:

```javascript
const presentation = [
  { slide: 1, title: "Overview", content: "..." },
  { slide: 2, title: "Key Findings", content: "..." },
  { slide: 3, title: "Recommendations", content: "..." }
];

const content = JSON.stringify(presentation);
// Output file name: analysis.pptx will trigger PowerPoint conversion
```

## File Naming Convention

The **file extension drives the conversion format**:

| Filename | Conversion |
|----------|-----------|
| `report.xlsx` | → Excel spreadsheet |
| `report.pdf` | → PDF document |
| `report.docx` | → Word document |
| `report.pptx` | → PowerPoint presentation |
| `data.csv` | → CSV spreadsheet |
| `data.json` | → JSON file |

## How It Works

1. **Agent generates JSON/structured data**
   ```javascript
   const myData = { /* structured data */ };
   const fileContent = JSON.stringify(myData);
   ```

2. **Agent specifies output filename with extension**
   - File goes into `executeCoder` with filename like `forecast.xlsx`

3. **Orchestrator detects extension and converts**
   - Orchestrator imports FileConverterClient
   - Calls `converter.toExcel()`, `converter.toPdf()`, etc.
   - Generates proper binary file

4. **File exported to user's filesystem**
   - Files are binary-correct and usable
   - No corruption, proper formatting

## Data Formats

### For Excel (.xlsx)
```javascript
// Array of objects (best):
const data = [
  { Month: "Jan", Revenue: 100000, Expenses: 30000 },
  { Month: "Feb", Revenue: 120000, Expenses: 36000 }
];

// Or object with key-value pairs:
const data = {
  "Total Revenue": 1000000,
  "Total Expenses": 300000,
  "Profit": 700000
};
```

### For PDF
```javascript
// Simple string:
const text = "This is my report...";

// Or structured data (will be formatted):
const data = {
  title: "Report Title",
  sections: ["Section 1 content", "Section 2 content"]
};
```

### For Word (.docx)
```javascript
// String content:
const text = "Document content...";

// Structured data with sections:
const data = {
  title: "Document Title",
  author: "Agent Name",
  sections: [...]
};
```

### For PowerPoint (.pptx)
```javascript
// Array of slide content:
const slides = [
  "Title slide content",
  "Slide 2 bullet points and content",
  "Slide 3 summary"
];

// Or array of objects:
const slides = [
  { title: "Slide 1", content: "..." },
  { title: "Slide 2", content: "..." }
];
```

## Examples

### Financial Analyst Creating Forecast

```javascript
// Generate forecast data
const forecast = {
  company: "SaaS Startup",
  year_range: "2024-2028",
  projections: [
    { year: 2024, mrr: 50000, churn: 0.05, ltv: 5000 },
    { year: 2025, mrr: 75000, churn: 0.04, ltv: 6250 }
  ]
};

// Return with .xlsx extension
files['forecast.xlsx'] = JSON.stringify(forecast);
```

### Writer Creating Report

```javascript
// Generate report structure
const report = {
  title: "Market Analysis",
  date: new Date().toISOString(),
  executive_summary: "...",
  findings: ["Finding 1", "Finding 2", "Finding 3"],
  recommendations: ["Rec 1", "Rec 2"]
};

// Return with .docx extension
files['report.docx'] = JSON.stringify(report);
```

### Data Scientist Creating Presentation

```javascript
// Generate slide data
const presentation = {
  title: "Data Analysis Results",
  slides: [
    { title: "Overview", content: "..." },
    { title: "Key Metrics", content: "..." },
    { title: "Conclusions", content: "..." }
  ]
};

// Return with .pptx extension
files['presentation.pptx'] = JSON.stringify(presentation);
```

## Troubleshooting

**Problem**: Files still get corrupted
**Solution**: Ensure you're returning JSON/structured data, not trying to create binary format manually

**Problem**: File has wrong extension after export
**Solution**: Check the filename returned from orchestrator - orchestrator detects `.xlsx`, `.pdf`, etc.

**Problem**: Converter says "unsupported format"
**Solution**: Use only: xlsx, pdf, docx, pptx, csv, json

## Best Practices

1. ✅ **Structure your data as JSON/objects**
   ```javascript
   // Good - structured
   const data = { rows: [...], headers: [...] };
   ```

2. ✅ **Use standard, clean data types**
   ```javascript
   // Good - clean data
   { year: 2024, revenue: 1000000, name: "Q1" }
   ```

3. ✅ **Name files with correct extensions**
   ```javascript
   // Good
   files['financial_model.xlsx'] = content;
   
   // Bad - no extension
   files['financial_model'] = content;
   ```

4. ❌ **Don't try to create binary files manually**
   ```javascript
   // Bad - doesn't work
   const xlsx = toBuffer(data); // Won't work
   files['model.xlsx'] = xlsx;
   ```

5. ❌ **Don't include control characters in JSON**
   ```javascript
   // Bad
   const data = "Line1\x00Line2"; // Control chars
   
   // Good
   const data = "Line1\nLine2"; // Standard escape
   ```

## Summary

Use the MCP File Converter by:
1. Creating structured JSON/data objects
2. Converting to JSON strings
3. Returning with correct file extensions
4. Let orchestrator handle the conversion

This ensures all files are properly formatted and usable!
