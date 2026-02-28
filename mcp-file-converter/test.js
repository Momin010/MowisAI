#!/usr/bin/env node

// Test the file converter MCP

import FileConverterClient from './client.js';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

async function test() {
  console.log('🧪 Testing MCP File Converter...\n');

  const converter = new FileConverterClient();

  // Test data
  const testData = {
    financial: [
      { Year: 2024, Revenue: 1000000, Expenses: 300000, Profit: 700000 },
      { Year: 2025, Revenue: 1500000, Expenses: 450000, Profit: 1050000 },
      { Year: 2026, Revenue: 2250000, Expenses: 675000, Profit: 1575000 }
    ],
    reportText: 'This is a sample financial report for a SaaS startup showing strong growth trajectory.'
  };

  const testDir = path.join(__dirname, 'test-output');
  if (!fs.existsSync(testDir)) {
    fs.mkdirSync(testDir, { recursive: true });
  }

  try {
    // Test Excel conversion
    console.log('📊 Testing Excel conversion...');
    const excelResult = await converter.toExcel(testData.financial, 'test.xlsx');
    if (excelResult.success) {
      const buffer = Buffer.from(excelResult.data, 'base64');
      const excelPath = path.join(testDir, 'test.xlsx');
      fs.writeFileSync(excelPath, buffer);
      console.log(`✅ Excel file created: ${excelPath} (${buffer.length} bytes)\n`);
    } else {
      console.log(`❌ Excel conversion failed: ${excelResult.error}\n`);
    }

    // Test PDF conversion
    console.log('📄 Testing PDF conversion...');
    const pdfResult = await converter.toPDF(testData.reportText, 'test.pdf');
    if (pdfResult.success) {
      const buffer = Buffer.from(pdfResult.data, 'base64');
      const pdfPath = path.join(testDir, 'test.pdf');
      fs.writeFileSync(pdfPath, buffer);
      console.log(`✅ PDF file created: ${pdfPath} (${buffer.length} bytes)\n`);
    } else {
      console.log(`❌ PDF conversion failed: ${pdfResult.error}\n`);
    }

    // Test Word conversion
    console.log('📝 Testing Word conversion...');
    const docxResult = await converter.toDocx(testData, 'test.docx');
    if (docxResult.success) {
      const buffer = Buffer.from(docxResult.data, 'base64');
      const docxPath = path.join(testDir, 'test.docx');
      fs.writeFileSync(docxPath, buffer);
      console.log(`✅ Word document created: ${docxPath} (${buffer.length} bytes)\n`);
    } else {
      console.log(`❌ Word conversion failed: ${docxResult.error}\n`);
    }

    // Test PowerPoint conversion
    console.log('🎯 Testing PowerPoint conversion...');
    const pptxResult = await converter.toPptx(testData.financial, 'test.pptx');
    if (pptxResult.success) {
      const buffer = Buffer.from(pptxResult.data, 'base64');
      const pptxPath = path.join(testDir, 'test.pptx');
      fs.writeFileSync(pptxPath, buffer);
      console.log(`✅ PowerPoint file created: ${pptxPath} (${buffer.length} bytes)\n`);
    } else {
      console.log(`❌ PowerPoint conversion failed: ${pptxResult.error}\n`);
    }

    // Test CSV conversion
    console.log('📋 Testing CSV conversion...');
    const csvResult = await converter.toCSV(testData.financial, 'test.csv');
    if (csvResult.success) {
      const buffer = Buffer.from(csvResult.data, 'base64');
      const csvPath = path.join(testDir, 'test.csv');
      fs.writeFileSync(csvPath, buffer);
      console.log(`✅ CSV file created: ${csvPath} (${buffer.length} bytes)\n`);
    } else {
      console.log(`❌ CSV conversion failed: ${csvResult.error}\n`);
    }

    console.log(`\n✨ All tests complete! Files saved to: ${testDir}`);
    console.log(`\nAvailable tools:`);
    const tools = converter.getAvailableTools();
    tools.forEach(tool => {
      console.log(`  • ${tool.name}`);
    });

  } catch (err) {
    console.error(`\n❌ Test error: ${err.message}`);
    console.error(err.stack);
  }
}

test();
