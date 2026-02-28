# MCP File Converter Server

Convert structured data (JSON, arrays, objects) to properly formatted file types.

## Features

- ✅ **Excel** (.xlsx) - Tables with headers, formatting
- ✅ **PDF** - Formatted documents with text layout
- ✅ **Word** (.docx) - Formatted documents with tables
- ✅ **PowerPoint** (.pptx) - Slide presentations
- ✅ **CSV** - Tabular data
- ✅ **JSON** - Structured data

## Installation

```bash
cd mcp-file-converter
npm install
```

## Tools Available

### 1. `convert_to_excel`
Convert data to Excel format (.xlsx)

**Input:**
```json
{
  "data": "[{\"name\": \"John\", \"age\": 30}, {\"name\": \"Jane\", \"age\": 28}]",
  "filename": "report.xlsx"
}
```

**Output:**
```json
{
  "success": true,
  "filename": "report.xlsx",
  "format": "xlsx",
  "size": 5240,
  "data": "base64_encoded_file_content"
}
```

### 2. `convert_to_pdf`
Convert data to PDF format

**Input:**
```json
{
  "data": "This is my report content",
  "filename": "report.pdf"
}
```

### 3. `convert_to_docx`
Convert data to Word document format (.docx)

**Input:**
```json
{
  "data": "{\"title\": \"My Report\", \"content\": \"Full content here\"}",
  "filename": "report.docx"
}
```

### 4. `convert_to_pptx`
Convert data to PowerPoint presentation (.pptx)

**Input:**
```json
{
  "data": "[\"Slide 1 content\", \"Slide 2 content\"]",
  "filename": "presentation.pptx"
}
```

### 5. `convert_to_csv`
Convert data to CSV format

**Input:**
```json
{
  "data": "[{\"id\": 1, \"name\": \"Item1\"}, {\"id\": 2, \"name\": \"Item2\"}]"
}
```

### 6. `convert_format`
Generic converter - specify format

**Input:**
```json
{
  "data": "your_data_here",
  "format": "pdf",
  "filename": "output.pdf"
}
```

## Agent Usage

Agents should use these tools to ensure proper file formatting:

```javascript
// In agent code:
const result = await mcp.callTool('convert_to_excel', {
  data: JSON.stringify(financialData),
  filename: 'forecast.xlsx'
});

if (result.success) {
  // Write the base64 decoded file
  const buffer = Buffer.from(result.data, 'base64');
  fs.writeFileSync(result.filename, buffer);
}
```

## Orchestrator Integration

The orchestrator can use the MCP tools to convert generated content:

```javascript
async function convertAndExport(content, targetFormat, filename) {
  const result = await mcp.callTool(`convert_to_${targetFormat}`, {
    data: JSON.stringify(content),
    filename: filename
  });
  
  return result;
}
```

## Supported Formats

| Format | Extension | Best For |
|--------|-----------|----------|
| Excel | .xlsx | Tables, spreadsheets, financial data |
| PDF | .pdf | Documents, reports, formatted text |
| Word | .docx | Formatted documents, text with styling |
| PowerPoint | .pptx | Presentations, slides |
| CSV | .csv | Data export, simple tables |
| JSON | .json | Data interchange, raw data |

## Return Format

All tools return:

```json
{
  "success": true/false,
  "filename": "output_filename.ext",
  "format": "format_type",
  "size": 1024,
  "data": "base64_encoded_content",
  "error": "Error message if failed"
}
```

The `data` field is base64 encoded. Decode it to write to filesystem:

```javascript
const buffer = Buffer.from(result.data, 'base64');
fs.writeFileSync(result.filename, buffer);
```

## Performance Notes

- Large datasets (1000+ rows) work fine
- PDF with rich formatting may take 1-2 seconds
- Excel with formulas/charts should be pre-computed as values
- PowerPoint scales to ~50 slides efficiently
