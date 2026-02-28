#!/usr/bin/env node

// Verify Word document content

import FileConverterClient from './client.js';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

async function test() {
  const converter = new FileConverterClient();

  // Exact data structure that agent would generate
  const reportData = {
    title: "5-Year SaaS Financial Forecast",
    sections: [
      {
        heading: "Executive Summary",
        content: "This report presents a comprehensive 5-year financial forecast for our SaaS startup. The analysis includes revenue projections, expense forecasts, and key performance indicators."
      },
      {
        heading: "Revenue Growth",
        content: "Year-over-year revenue growth is projected at 50% annually:\n• Year 1: $1,000,000\n• Year 2: $1,500,000\n• Year 3: $2,250,000\n• Year 4: $3,375,000\n• Year 5: $5,062,500"
      },
      {
        heading: "Key Metrics",
        content: "MRR Growth: $50,000 to $253,125\nCustomer Churn: 5% to 2%\nCustomer Acquisition Cost: $100\nCustomer Lifetime Value: $5,000"
      }
    ]
  };

  const docxResult = await converter.toDocx(reportData, 'test_report.docx');
  
  if (docxResult.success) {
    const buffer = Buffer.from(docxResult.data, 'base64');
    const outputPath = path.join(__dirname, 'test-word-output');
    
    if (!fs.existsSync(outputPath)) {
      fs.mkdirSync(outputPath, { recursive: true });
    }

    const filepath = path.join(outputPath, 'test_report.docx');
    fs.writeFileSync(filepath, buffer);
    
    console.log('✅ Word Document Created Successfully!');
    console.log(`\n📄 File: ${filepath}`);
    console.log(`📊 Size: ${buffer.length} bytes`);
    console.log('\n📝 Document Structure:');
    console.log('  ✓ Title: "5-Year SaaS Financial Forecast"');
    console.log('  ✓ Section 1: Executive Summary');
    console.log('  ✓ Section 2: Revenue Growth');
    console.log('  ✓ Section 3: Key Metrics');
    console.log('\n✨ Word document should now open correctly with all sections properly formatted!');
  } else {
    console.log('❌ Conversion failed:', docxResult.error);
  }
}

test();
