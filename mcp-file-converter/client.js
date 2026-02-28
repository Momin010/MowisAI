#!/usr/bin/env node

// MCP File Converter Client
// This module allows other services to call the file converter MCP tools

import { FileConverterMCP, convertFile } from './server.js';
import fs from 'fs';
import path from 'path';

class FileConverterClient {
  constructor() {
    this.mcp = new FileConverterMCP();
  }

  /**
   * Convert data to specified format
   * Returns the converted file as a Buffer
   */
  async convert(data, format, filename) {
    return this.mcp.executeTool(`convert_to_${format}`, {
      data: typeof data === 'string' ? data : JSON.stringify(data),
      filename: filename
    });
  }

  /**
   * Convert and save to file
   */
  async convertAndSave(data, format, outputPath) {
    try {
      const result = await this.convert(data, format);
      
      if (!result.success) {
        throw new Error(result.error);
      }

      // Decode base64 and write file
      const buffer = Buffer.from(result.data, 'base64');
      
      // Create directory if needed
      const dir = path.dirname(outputPath);
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
      }

      fs.writeFileSync(outputPath, buffer);
      
      return {
        success: true,
        filename: path.basename(outputPath),
        path: outputPath,
        size: buffer.length
      };
    } catch (err) {
      return {
        success: false,
        error: err.message
      };
    }
  }

  /**
   * Get list of available tools
   */
  getAvailableTools() {
    return this.mcp.getToolDefinitions();
  }

  /**
   * Convert Excel-like data to .xlsx
   */
  async toExcel(data, filename = 'output.xlsx') {
    return this.convert(data, 'xlsx', filename);
  }

  /**
   * Convert text/data to PDF
   */
  async toPDF(data, filename = 'output.pdf') {
    return this.convert(data, 'pdf', filename);
  }

  /**
   * Convert to Word document
   */
  async toDocx(data, filename = 'output.docx') {
    return this.convert(data, 'docx', filename);
  }

  /**
   * Convert to PowerPoint
   */
  async toPptx(data, filename = 'output.pptx') {
    return this.convert(data, 'pptx', filename);
  }

  /**
   * Convert to CSV
   */
  async toCSV(data, filename = 'output.csv') {
    return this.convert(data, 'csv', filename);
  }

  /**
   * Batch convert files
   */
  async convertBatch(items) {
    const results = [];
    for (const item of items) {
      const result = await this.convert(item.data, item.format, item.filename);
      results.push(result);
    }
    return results;
  }
}

export default FileConverterClient;
export { FileConverterMCP, convertFile };
