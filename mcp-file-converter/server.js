#!/usr/bin/env node

import ExcelJS from 'exceljs';
import PDFDocument from 'pdfkit';
import { Document, Packer, Paragraph, Table, TableCell, TextRun, BorderStyle } from 'docx';
import PptxGenJs from 'pptxgenjs';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import { Readable } from 'stream';
import {
  generatePieChart,
  generateBarChart,
  generateLineChart,
  generateComparisonChart,
  generateDoughnutChart
} from './chart-generator.js';
import {
  getImageForDocument,
  processImages,
  createGradient,
  addTextOverlay,
  createCollage
} from './image-handler.js';
import {
  searchWeb,
  fetchJSON,
  fetchStockData,
  fetchWeather,
  fetchCryptoPrices,
  fetchGitHubTrending,
  fetchNews
} from './data-fetcher.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/**
 * Smart data parser - extract structure from various inputs
 */
function smartParse(data) {
  if (!data) return { type: 'empty', value: '' };

  // Already structured
  if (typeof data === 'object') {
    return { type: 'object', value: data };
  }

  // Try JSON parse
  if (typeof data === 'string') {
    try {
      const parsed = JSON.parse(data);
      return { type: 'object', value: parsed };
    } catch (e) {
      // Not JSON
    }

    // Try to extract table-like data
    const lines = data.split('\n').filter(l => l.trim());
    
    // Check if it looks like CSV/table
    if (lines.length > 1) {
      const firstLine = lines[0];
      const hasDelimiter = firstLine.includes(',') || firstLine.includes('|') || firstLine.includes('\t');
      
      if (hasDelimiter) {
        const delimiter = firstLine.includes('\t') ? '\t' : firstLine.includes('|') ? '|' : ',';
        const rows = lines.map(line => {
          const cells = line.split(delimiter).map(c => c.trim());
          return cells;
        });
        return { type: 'table', value: rows };
      }
    }

    // Treat as plain text
    return { type: 'text', value: data };
  }

  return { type: 'unknown', value: String(data) };
}

/**
 * Convert structured data to Excel format (.xlsx)
 */
async function convertToExcel(data, filename = 'output.xlsx') {
  const workbook = new ExcelJS.Workbook();
  const worksheet = workbook.addWorksheet('Data');

  const parsed = smartParse(data);

  if (parsed.type === 'table') {
    // Table data
    const [headers, ...rows] = parsed.value;
    worksheet.columns = headers.map((h, i) => ({
      header: h,
      key: `col${i}`,
      width: 20
    }));

    rows.forEach(row => {
      const rowData = {};
      headers.forEach((h, i) => {
        rowData[`col${i}`] = row[i] || '';
      });
      worksheet.addRow(rowData);
    });

    // Style header row
    worksheet.getRow(1).font = { bold: true, size: 12 };
    worksheet.getRow(1).fill = { type: 'pattern', pattern: 'solid', fgColor: { argb: 'FFD3D3D3' } };
  } else if (parsed.type === 'object') {
    const obj = parsed.value;

    // Check if it's an array of objects (best case)
    if (Array.isArray(obj) && obj.length > 0 && typeof obj[0] === 'object') {
      const headers = Object.keys(obj[0]);
      worksheet.columns = headers.map(h => ({ header: h, key: h, width: 18 }));

      obj.forEach(item => {
        worksheet.addRow(item);
      });

      // Style header
      worksheet.getRow(1).font = { bold: true, size: 12 };
      worksheet.getRow(1).fill = { type: 'pattern', pattern: 'solid', fgColor: { argb: 'FF4472C4' } };
      worksheet.getRow(1).font.color = { argb: 'FFFFFFFF' };
    } else if (Array.isArray(obj)) {
      // Array of primitives
      worksheet.columns = [{ header: 'Item', key: 'item', width: 30 }];
      obj.forEach(item => worksheet.addRow({ item }));
    } else {
      // Key-value pairs
      const entries = Object.entries(obj);
      worksheet.columns = [
        { header: 'Property', key: 'prop', width: 25 },
        { header: 'Value', key: 'val', width: 40 }
      ];

      entries.forEach(([k, v]) => {
        worksheet.addRow({
          prop: k,
          val: typeof v === 'object' ? JSON.stringify(v) : v
        });
      });

      // Style header
      worksheet.getRow(1).font = { bold: true, size: 12 };
      worksheet.getRow(1).fill = { type: 'pattern', pattern: 'solid', fgColor: { argb: 'FF4472C4' } };
      worksheet.getRow(1).font.color = { argb: 'FFFFFFFF' };
    }
  } else {
    // Text data - split by lines/paragraphs
    worksheet.columns = [{ header: 'Content', key: 'content', width: 50 }];
    
    const paragraphs = parsed.value.split(/\n\n+/).filter(p => p.trim());
    paragraphs.forEach(para => {
      worksheet.addRow({ content: para.trim() });
    });

    // Style header
    worksheet.getRow(1).font = { bold: true };
  }

  const buffer = await workbook.xlsx.writeBuffer();
  return buffer;
}

/**
 * Convert structured data to PDF format
 */
async function convertToPDF(data, filename = 'output.pdf') {
  return new Promise((resolve, reject) => {
    const chunks = [];
    const doc = new PDFDocument();

    doc.on('data', chunk => chunks.push(chunk));
    doc.on('end', () => resolve(Buffer.concat(chunks)));
    doc.on('error', reject);

    // Add content based on data type
    if (typeof data === 'string') {
      doc.fontSize(12);
      doc.text(data, { align: 'left', width: 500 });
    } else if (Array.isArray(data)) {
      doc.fontSize(14).text('Data Table', { underline: true });
      doc.fontSize(10);
      data.forEach((item, index) => {
        if (typeof item === 'object') {
          doc.text(`Row ${index + 1}:`, { underline: true });
          Object.entries(item).forEach(([key, value]) => {
            doc.text(`  ${key}: ${value}`);
          });
        } else {
          doc.text(`${index + 1}. ${item}`);
        }
        doc.moveDown(0.5);
      });
    } else if (typeof data === 'object') {
      doc.fontSize(14).text('Data', { underline: true });
      doc.fontSize(10);
      Object.entries(data).forEach(([key, value]) => {
        if (typeof value === 'object') {
          doc.text(`${key}:`, { underline: true });
          Object.entries(value).forEach(([k, v]) => {
            doc.text(`  ${k}: ${v}`);
          });
        } else {
          doc.text(`${key}: ${value}`);
        }
        doc.moveDown(0.3);
      });
    }

    doc.end();
  });
}

/**
 * Convert structured data to Word document format (.docx)
 */
async function convertToDocx(data, filename = 'output.docx') {
  const sections = [];

  const parsed = smartParse(data);

  if (parsed.type === 'object') {
    const obj = parsed.value;

    // Extract title if present
    if (obj.title) {
      sections.push(
        new Paragraph({
          text: obj.title,
          heading: 'Heading1',
          spacing: { before: 0, after: 400 }
        })
      );
    } else {
      sections.push(
        new Paragraph({
          text: 'Generated Document',
          heading: 'Heading1',
          spacing: { before: 0, after: 400 }
        })
      );
    }

    // Handle sections array (most common structure)
    if (obj.sections && Array.isArray(obj.sections)) {
      obj.sections.forEach((section, idx) => {
        if (typeof section === 'object' && section.heading) {
          // Section with heading and content
          sections.push(
            new Paragraph({
              text: section.heading,
              heading: 'Heading2',
              spacing: { before: 200, after: 100 }
            })
          );

          if (section.content) {
            // Split content by lines for better formatting
            const lines = String(section.content).split('\n');
            lines.forEach(line => {
              if (line.trim()) {
                sections.push(
                  new Paragraph({
                    text: line.trim(),
                    spacing: { after: 80, line: 240, lineRule: 'auto' }
                  })
                );
              }
            });
          }

          sections.push(new Paragraph({ text: '' })); // Empty line between sections
        } else if (typeof section === 'string') {
          // Plain text section
          sections.push(
            new Paragraph({
              text: section,
              spacing: { after: 200, line: 240, lineRule: 'auto' }
            })
          );
        }
      });
    }

    // Handle other properties
    Object.entries(obj).forEach(([key, value]) => {
      if (key === 'title' || key === 'sections') return; // Already handled

      if (typeof value === 'object' && !Array.isArray(value)) {
        // Nested object
        sections.push(
          new Paragraph({
            text: key,
            heading: 'Heading2',
            spacing: { before: 200, after: 100 }
          })
        );

        Object.entries(value).forEach(([k, v]) => {
          sections.push(
            new Paragraph({
              children: [
                new TextRun({ text: k + ': ', bold: true }),
                new TextRun(String(v))
              ],
              spacing: { after: 80 }
            })
          );
        });
      } else if (Array.isArray(value)) {
        // Array of items
        sections.push(
          new Paragraph({
            text: key,
            heading: 'Heading2',
            spacing: { before: 200, after: 100 }
          })
        );

        value.forEach((item, i) => {
          if (typeof item === 'object') {
            // Object in array
            sections.push(
              new Paragraph({
                text: `${i + 1}. ${Object.values(item).join(', ')}`,
                spacing: { after: 80 }
              })
            );
          } else {
            // String or primitive
            sections.push(
              new Paragraph({
                text: `${i + 1}. ${item}`,
                spacing: { after: 80 }
              })
            );
          }
        });
      } else if (value) {
        // Simple property
        sections.push(
          new Paragraph({
            children: [
              new TextRun({ text: key + ': ', bold: true }),
              new TextRun(String(value))
            ],
            spacing: { after: 100 }
          })
        );
      }
    });
  } else if (parsed.type === 'table') {
    // Table data
    sections.push(
      new Paragraph({
        text: 'Generated Document',
        heading: 'Heading1',
        spacing: { before: 0, after: 400 }
      })
    );

    const [headers, ...rows] = parsed.value;

    const tableRows = [
      {
        cells: headers.map(h =>
          new TableCell({
            children: [new Paragraph({ text: h, bold: true })],
            shading: { type: 'clear', color: 'D3D3D3' }
          })
        )
      },
      ...rows.map(row => ({
        cells: row.map(cell =>
          new TableCell({
            children: [new Paragraph(String(cell))]
          })
        )
      }))
    ];

    sections.push(
      new Table({
        rows: tableRows,
        width: { size: 100, type: 'pct' }
      })
    );
  } else {
    // Text content - split into paragraphs
    sections.push(
      new Paragraph({
        text: 'Generated Document',
        heading: 'Heading1',
        spacing: { before: 0, after: 400 }
      })
    );

    const paragraphs = parsed.value.split(/\n\n+/).filter(p => p.trim());

    paragraphs.forEach(para => {
      sections.push(
        new Paragraph({
          text: para.trim(),
          spacing: { line: 240, after: 200, lineRule: 'auto' }
        })
      );
    });
  }

  const doc = new Document({ sections: [{ children: sections }] });
  const buffer = await Packer.toBuffer(doc);
  return buffer;
}

/**
 * Convert structured data to PowerPoint format (.pptx)
 */
async function convertToPptx(data, filename = 'output.pptx') {
  const pres = new PptxGenJs();

  // Title slide
  const slide1 = pres.addSlide();
  slide1.background = { color: '0056B3' };
  slide1.addText('Generated Presentation', {
    x: 0.5,
    y: 2.0,
    w: 9,
    h: 1,
    fontSize: 44,
    bold: true,
    color: 'FFFFFF',
    align: 'center'
  });

  const parsed = smartParse(data);

  // Content slides based on parsed data
  if (parsed.type === 'object') {
    const obj = parsed.value;

    // Array of objects or titles
    if (Array.isArray(obj)) {
      obj.forEach((item, idx) => {
        const slide = pres.addSlide();
        
        // Slide background
        slide.background = { color: 'FFFFFF' };

        // Alternate header colors
        const headerColor = idx % 2 === 0 ? '0056B3' : '1F4E78';
        
        // Title
        let title = '';
        if (typeof item === 'object' && item.title) {
          title = String(item.title);
        } else if (typeof item === 'object' && item.name) {
          title = String(item.name);
        } else if (typeof item === 'object' && item.heading) {
          title = String(item.heading);
        } else {
          title = `Slide ${idx + 1}`;
        }

        slide.addText(title, {
          x: 0.5,
          y: 0.3,
          w: 9,
          h: 0.7,
          fontSize: 32,
          bold: true,
          color: headerColor
        });

        // Content
        let content = '';
        if (typeof item === 'object') {
          const contentParts = [];
          Object.entries(item).forEach(([k, v]) => {
            if (k !== 'title' && k !== 'name' && k !== 'heading') {
              contentParts.push(`• ${k}: ${v}`);
            }
          });
          content = contentParts.join('\n');
        } else {
          content = String(item);
        }

        slide.addText(content, {
          x: 0.7,
          y: 1.3,
          w: 8.6,
          h: 4.7,
          fontSize: 18,
          wrap: true,
          lineSpacing: 28,
          valign: 'top'
        });
      });
    } else if (typeof obj === 'object') {
      // Object with key-value pairs - create sections
      const entries = Object.entries(obj);
      
      entries.forEach(([key, value], idx) => {
        const slide = pres.addSlide();
        slide.background = { color: 'FFFFFF' };

        // Title
        slide.addText(key, {
          x: 0.5,
          y: 0.3,
          w: 9,
          h: 0.7,
          fontSize: 32,
          bold: true,
          color: '0056B3'
        });

        // Content
        let content = '';
        if (Array.isArray(value)) {
          content = value.map(v => `• ${v}`).join('\n');
        } else if (typeof value === 'object') {
          content = Object.entries(value)
            .map(([k, v]) => `${k}: ${v}`)
            .join('\n');
        } else {
          content = String(value);
        }

        slide.addText(content, {
          x: 0.7,
          y: 1.3,
          w: 8.6,
          h: 4.7,
          fontSize: 16,
          wrap: true,
          lineSpacing: 24,
          valign: 'top'
        });
      });
    }
  } else if (parsed.type === 'table') {
    // Table data - create one slide with content
    const slide = pres.addSlide();
    slide.background = { color: 'FFFFFF' };

    slide.addText('Data Table', {
      x: 0.5,
      y: 0.3,
      w: 9,
      h: 0.6,
      fontSize: 28,
      bold: true,
      color: '0056B3'
    });

    const [headers, ...rows] = parsed.value;
    const content = [
      headers.join(' | '),
      '-'.repeat(50),
      ...rows.map(row => row.join(' | '))
    ].join('\n');

    slide.addText(content, {
      x: 0.7,
      y: 1.2,
      w: 8.6,
      h: 4.8,
      fontSize: 12,
      fontFace: 'Courier New'
    });
  } else {
    // Text content - split into slides
    const lines = parsed.value.split('\n').filter(l => l.trim());
    const chunked = [];
    
    for (let i = 0; i < lines.length; i += 10) {
      chunked.push(lines.slice(i, i + 10));
    }

    chunked.forEach((chunk, idx) => {
      const slide = pres.addSlide();
      slide.background = { color: 'FFFFFF' };

      slide.addText(`Content ${idx + 1}`, {
        x: 0.5,
        y: 0.3,
        w: 9,
        h: 0.6,
        fontSize: 28,
        bold: true,
        color: '0056B3'
      });

      const content = chunk.join('\n');
      slide.addText(content, {
        x: 0.7,
        y: 1.1,
        w: 8.6,
        h: 4.9,
        fontSize: 14,
        wrap: true,
        lineSpacing: 20,
        valign: 'top'
      });
    });
  }

  const buffer = await pres.write({ outputType: 'nodebuffer' });
  return buffer;
}

/**
 * Convert structured data to CSV format
 */
function convertToCSV(data) {
  if (Array.isArray(data)) {
    // Convert array of objects to CSV
    if (data.length === 0) return '';
    
    const headers = Object.keys(data[0]);
    const rows = data.map(item =>
      headers.map(h => {
        const value = item[h];
        if (typeof value === 'string' && (value.includes(',') || value.includes('"'))) {
          return `"${value.replace(/"/g, '""')}"`;
        }
        return value;
      }).join(',')
    );
    
    return [headers.join(','), ...rows].join('\n');
  } else if (typeof data === 'object') {
    // Convert object to CSV
    const lines = Object.entries(data).map(([k, v]) => {
      const escapedKey = typeof k === 'string' && k.includes(',') ? `"${k}"` : k;
      const escapedVal = typeof v === 'string' && v.includes(',') ? `"${v}"` : v;
      return `${escapedKey},${escapedVal}`;
    });
    return lines.join('\n');
  }
  return String(data);
}

/**
 * Convert structured data to JSON format
 */
function convertToJSON(data) {
  return JSON.stringify(data, null, 2);
}

/**
 * Generate chart based on config
 */
async function generateChart(config) {
  try {
    const { type = 'bar', data, title } = config;

    switch (type.toLowerCase()) {
      case 'pie':
        return await generatePieChart(data, title);
      case 'bar':
        return await generateBarChart(data, title);
      case 'line':
        return await generateLineChart(data, title);
      case 'doughnut':
        return await generateDoughnutChart(data, title);
      case 'comparison':
        return await generateComparisonChart(data, title);
      default:
        return await generateBarChart(data, title);
    }
  } catch (error) {
    console.error('Chart generation failed:', error.message);
    return null;
  }
}

/**
 * Enhanced Excel with charts
 */
async function convertToExcelWithCharts(data, charts = []) {
  const workbook = new ExcelJS.Workbook();
  const worksheet = workbook.addWorksheet('Data');

  const parsed = smartParse(data);

  // Add data
  if (parsed.type === 'object' && Array.isArray(parsed.value)) {
    const headers = Object.keys(parsed.value[0] || {});
    worksheet.columns = headers.map(h => ({ header: h, key: h, width: 18 }));
    parsed.value.forEach(item => worksheet.addRow(item));
    
    worksheet.getRow(1).font = { bold: true, size: 12 };
    worksheet.getRow(1).fill = { type: 'pattern', pattern: 'solid', fgColor: { argb: 'FF4472C4' } };
    worksheet.getRow(1).font.color = { argb: 'FFFFFFFF' };
  }

  // Add charts
  for (const chartConfig of charts) {
    try {
      const chartImg = await generateChart(chartConfig);
      if (chartImg) {
        const imageId = workbook.addImage({ buffer: chartImg, extension: 'png' });
        worksheet.addImage(imageId, `A${Math.max(10, (parsed.value?.length || 0) + 5)}`);
      }
    } catch (err) {
      console.warn('Failed to add chart to Excel:', err.message);
    }
  }

  const buffer = await workbook.xlsx.writeBuffer();
  return buffer;
}

/**
 * Enhanced PowerPoint with charts and images
 */
async function convertToPptxWithVisuals(data, charts = [], images = []) {
  const prs = new PptxGenJs();

  const parsed = smartParse(data);
  
  // Add content slides
  if (parsed.type === 'object' && Array.isArray(parsed.value)) {
    for (let i = 0; i < parsed.value.length; i += 5) {
      const slide = prs.addSlide();
      const chunk = parsed.value.slice(i, i + 5);
      
      slide.background = { color: 'FFFFFF' };
      
      let y = 0.5;
      chunk.forEach(item => {
        const content = typeof item === 'object' ? 
          Object.entries(item).map(([k, v]) => `${k}: ${v}`).join('\n') : 
          String(item);
        
        slide.addText(content, {
          x: 0.5, y, w: 9, h: 1,
          fontSize: 12,
          color: '333333',
          wrap: true
        });
        y += 1.2;
      });
    }
  }

  // Add chart slides
  for (const chartConfig of charts) {
    try {
      const slide = prs.addSlide();
      slide.background = { color: 'F5F5F5' };
      
      slide.addText(chartConfig.title || 'Chart', {
        x: 0.5, y: 0.3, w: 9, h: 0.5,
        fontSize: 20, bold: true, color: '333333'
      });

      const chartImg = await generateChart(chartConfig);
      if (chartImg) {
        slide.addImage({ data: chartImg, path: undefined, x: 1, y: 1, w: 8, h: 4 });
      }
    } catch (err) {
      console.warn('Failed to add chart to PowerPoint:', err.message);
    }
  }

  // Add image slide
  if (images.length > 0) {
    try {
      const slide = prs.addSlide();
      slide.background = { color: 'FFFFFF' };
      
      slide.addText('Images', {
        x: 0.5, y: 0.3, w: 9, h: 0.5,
        fontSize: 20, bold: true, color: '333333'
      });

      for (let i = 0; i < images.length; i++) {
        try {
          const img = await getImageForDocument(images[i], { width: 350, height: 250 });
          if (img) {
            slide.addImage({ data: img, path: undefined, 
              x: 0.5 + (i % 2) * 4.5, 
              y: 1 + Math.floor(i / 2) * 2.8, 
              w: 4, h: 3 
            });
          }
        } catch (err) {
          console.warn('Failed to add image:', err.message);
        }
      }
    } catch (err) {
      console.warn('Failed to add image slide:', err.message);
    }
  }

  const buffer = await prs.writeFile({ fileName: 'temp' });
  return buffer;
}

/**
 * Main conversion dispatcher
 */
async function convertFile(data, format, filename) {
  try {
    let buffer;

    switch (format.toLowerCase()) {
      case 'xlsx':
      case 'excel':
        buffer = await convertToExcel(data, filename);
        break;

      case 'pdf':
        buffer = await convertToPDF(data, filename);
        break;

      case 'docx':
      case 'word':
        buffer = await convertToDocx(data, filename);
        break;

      case 'pptx':
      case 'powerpoint':
        buffer = await convertToPptx(data, filename);
        break;

      case 'csv':
        buffer = Buffer.from(convertToCSV(data));
        break;

      case 'json':
        buffer = Buffer.from(convertToJSON(data));
        break;

      default:
        throw new Error(`Unsupported format: ${format}`);
    }

    // Return as base64
    return {
      success: true,
      filename: filename,
      format: format,
      size: buffer.length,
      data: buffer.toString('base64')
    };
  } catch (err) {
    return {
      success: false,
      error: err.message
    };
  }
}

/**
 * MCP Server Implementation
 */
class FileConverterMCP {
  constructor() {
    this.name = 'file-converter';
    this.version = '1.0.0';
  }

  getToolDefinitions() {
    return [
      {
        name: 'convert_to_excel',
        description: 'Convert structured data (JSON/array) to Excel spreadsheet (.xlsx)',
        inputSchema: {
          type: 'object',
          properties: {
            data: {
              type: 'string',
              description: 'JSON string containing the data to convert'
            },
            filename: {
              type: 'string',
              description: 'Output filename (default: output.xlsx)'
            }
          },
          required: ['data']
        }
      },
      {
        name: 'convert_to_pdf',
        description: 'Convert structured data to PDF document',
        inputSchema: {
          type: 'object',
          properties: {
            data: {
              type: 'string',
              description: 'Content or JSON string to convert to PDF'
            },
            filename: {
              type: 'string',
              description: 'Output filename (default: output.pdf)'
            }
          },
          required: ['data']
        }
      },
      {
        name: 'convert_to_docx',
        description: 'Convert structured data to Word document (.docx)',
        inputSchema: {
          type: 'object',
          properties: {
            data: {
              type: 'string',
              description: 'JSON string or text content to convert'
            },
            filename: {
              type: 'string',
              description: 'Output filename (default: output.docx)'
            }
          },
          required: ['data']
        }
      },
      {
        name: 'convert_to_pptx',
        description: 'Convert structured data to PowerPoint presentation (.pptx)',
        inputSchema: {
          type: 'object',
          properties: {
            data: {
              type: 'string',
              description: 'JSON string or content array to convert'
            },
            filename: {
              type: 'string',
              description: 'Output filename (default: output.pptx)'
            }
          },
          required: ['data']
        }
      },
      {
        name: 'convert_to_csv',
        description: 'Convert structured data to CSV format',
        inputSchema: {
          type: 'object',
          properties: {
            data: {
              type: 'string',
              description: 'JSON array or object to convert to CSV'
            },
            filename: {
              type: 'string',
              description: 'Output filename (default: output.csv)'
            }
          },
          required: ['data']
        }
      },
      {
        name: 'convert_format',
        description: 'Generic file format converter - convert data to any supported format',
        inputSchema: {
          type: 'object',
          properties: {
            data: {
              type: 'string',
              description: 'Content or JSON data to convert'
            },
            format: {
              type: 'string',
              enum: ['xlsx', 'pdf', 'docx', 'pptx', 'csv', 'json'],
              description: 'Target format'
            },
            filename: {
              type: 'string',
              description: 'Output filename'
            }
          },
          required: ['data', 'format']
        }
      }
    ];
  }

  async executeTool(name, args) {
    try {
      let data;
      const filename = args.filename || 'output';

      // Parse JSON if data is a string
      try {
        data = JSON.parse(args.data);
      } catch {
        data = args.data;
      }

      let format;
      if (name.startsWith('convert_to_')) {
        format = name.replace('convert_to_', '');
      } else if (name === 'convert_format') {
        format = args.format;
      }

      const result = await convertFile(data, format, filename);
      return result;
    } catch (err) {
      return {
        success: false,
        error: err.message
      };
    }
  }
}

// Standalone server mode (can be extended for MCP protocol)
const server = new FileConverterMCP();

// Banner output removed to avoid leaking server internals into agent outputs.
// Use process.env.MCP_NO_BANNER if explicit control needed in the future.

// Export for use as module
export { 
  FileConverterMCP, 
  convertFile, 
  convertToExcel, 
  convertToPDF, 
  convertToDocx, 
  convertToPptx, 
  convertToCSV,
  convertToExcelWithCharts,
  convertToPptxWithVisuals,
  generateChart
};
