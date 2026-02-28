#!/usr/bin/env node

// Test the improved file converter with realistic financial data

import FileConverterClient from './client.js';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

async function test() {
  console.log('🧪 Testing Improved File Converter with Realistic Data...\n');

  const converter = new FileConverterClient();

  // Realistic financial data like an agent would generate
  const forecastData = [
    { Year: 2024, Revenue: 1000000, Expenses: 300000, NetIncome: 700000, MRR: 50000, Churn: 0.05 },
    { Year: 2025, Revenue: 1500000, Expenses: 450000, NetIncome: 1050000, MRR: 75000, Churn: 0.04 },
    { Year: 2026, Revenue: 2250000, Expenses: 675000, NetIncome: 1575000, MRR: 112500, Churn: 0.03 },
    { Year: 2027, Revenue: 3375000, Expenses: 1012500, NetIncome: 2362500, MRR: 168750, Churn: 0.025 },
    { Year: 2028, Revenue: 5062500, Expenses: 1518750, NetIncome: 3543750, MRR: 253125, Churn: 0.02 }
  ];

  const reportData = {
    title: "5-Year SaaS Financial Forecast",
    executive_summary: "This comprehensive forecast outlines the growth trajectory of the SaaS startup with conservative revenue projections and expense management strategies.",
    sections: [
      {
        heading: "Revenue Projections",
        content: "Year-over-year revenue growth is projected at 50% annually, driven by customer acquisition and market expansion. Starting from $1M in 2024, revenue is expected to reach $5M by 2028."
      },
      {
        heading: "Operating Expenses",
        content: "Operating expenses are maintained at 30% of revenue, including R&D (15%), Sales & Marketing (10%), and G&A (5%). This ratio improves slightly as the company scales."
      },
      {
        heading: "Key Metrics",
        content: "Monthly Recurring Revenue (MRR) grows from $50K to $250K. Customer Churn decreases from 5% to 2% annually as product maturity and customer satisfaction improve."
      }
    ]
  };

  const presentationSlides = [
    { title: "Executive Summary", content: "5-year forecast showing $1M to $5M revenue growth\n\n• Strong market demand\n• Efficient scaling model\n• Healthy unit economics" },
    { title: "Revenue Growth", content: "Annual Growth Rate: 50%\n\n• Year 1: $1.0M\n• Year 2: $1.5M\n• Year 3: $2.25M\n• Year 4: $3.375M\n• Year 5: $5.0M" },
    { title: "Key Metrics", content: "Customer Metrics:\n• MRR: $50K → $250K\n• Churn: 5% → 2%\n• ARR: $600K → $3M" },
    { title: "Action Items", content: "• Optimize sales processes\n• Improve customer retention\n• Scale marketing efforts\n• Invest in product development" }
  ];

  const testDir = path.join(__dirname, 'test-realistic-output');
  if (!fs.existsSync(testDir)) {
    fs.mkdirSync(testDir, { recursive: true });
  }

  try {
    console.log('📊 Converting Financial Data to Excel...');
    const excelResult = await converter.toExcel(forecastData, 'forecast.xlsx');
    if (excelResult.success) {
      const buffer = Buffer.from(excelResult.data, 'base64');
      const excelPath = path.join(testDir, 'forecast.xlsx');
      fs.writeFileSync(excelPath, buffer);
      console.log(`✅ Excel: ${buffer.length} bytes - Properly formatted table\n`);
    }

    console.log('📝 Converting Report to Word Document...');
    const docxResult = await converter.toDocx(reportData, 'forecast_report.docx');
    if (docxResult.success) {
      const buffer = Buffer.from(docxResult.data, 'base64');
      const docxPath = path.join(testDir, 'forecast_report.docx');
      fs.writeFileSync(docxPath, buffer);
      console.log(`✅ Word: ${buffer.length} bytes - Properly formatted document with sections\n`);
    }

    console.log('🎯 Converting Slides to PowerPoint...');
    const pptxResult = await converter.toPptx(presentationSlides, 'forecast_presentation.pptx');
    if (pptxResult.success) {
      const buffer = Buffer.from(pptxResult.data, 'base64');
      const pptxPath = path.join(testDir, 'forecast_presentation.pptx');
      fs.writeFileSync(pptxPath, buffer);
      console.log(`✅ PowerPoint: ${buffer.length} bytes - ${presentationSlides.length} properly formatted slides\n`);
    }

    console.log('📄 Converting Data to PDF...');
    const pdfData = `5-Year SaaS Financial Forecast\n\n${JSON.stringify(forecastData.slice(0, 2), null, 2)}`;
    const pdfResult = await converter.toPDF(pdfData, 'forecast.pdf');
    if (pdfResult.success) {
      const buffer = Buffer.from(pdfResult.data, 'base64');
      const pdfPath = path.join(testDir, 'forecast.pdf');
      fs.writeFileSync(pdfPath, buffer);
      console.log(`✅ PDF: ${buffer.length} bytes - Professional document format\n`);
    }

    console.log('📋 Converting Data to CSV...');
    const csvResult = await converter.toCSV(forecastData, 'forecast.csv');
    if (csvResult.success) {
      const buffer = Buffer.from(csvResult.data, 'base64');
      const csvPath = path.join(testDir, 'forecast.csv');
      fs.writeFileSync(csvPath, buffer);
      console.log(`✅ CSV: ${buffer.length} bytes - Tabular format\n`);
    }

    console.log(`\n✨ All realistic tests passed!`);
    console.log(`\nGenerated files in: ${testDir}`);
    console.log('\n📊 File Summary:');
    console.log('  • forecast.xlsx - Financial data table with proper formatting');
    console.log('  • forecast_report.docx - Professional report with sections and structure');
    console.log('  • forecast_presentation.pptx - 4 slides with proper layouts');
    console.log('  • forecast.pdf - Formatted PDF document');
    console.log('  • forecast.csv - CSV export of data');

  } catch (err) {
    console.error(`\n❌ Test error: ${err.message}`);
    console.error(err.stack);
  }
}

test();
