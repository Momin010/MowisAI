#!/usr/bin/env node

// Comprehensive Professional Features Test
import {
  convertToExcelWithCharts,
  convertToPptxWithVisuals,
  convertFile
} from './server.js';
import {
  generatePieChart,
  generateBarChart,
  generateLineChart,
  generateDoughnutChart
} from './chart-generator.js';
import {
  getImageForDocument,
  createCollage
} from './image-handler.js';
import {
  searchWeb,
  fetchWeather,
  fetchGitHubTrending,
  fetchNews
} from './data-fetcher.js';
import fs from 'fs';

console.log('🚀 Professional Features Test Suite\n');

// Test 1: Web Search
console.log('📡 Test 1: Web Search');
const searchResults = await searchWeb('artificial intelligence', { limit: 3 });
console.log(`✓ Found ${searchResults.length} results for AI search`);

// Test 2: Weather Data
console.log('\n🌤️ Test 2: Weather Data');
const weather = await fetchWeather('New York');
if (weather) {
  console.log(`✓ Location: ${weather.location}`);
  console.log(`✓ Temperature: ${weather.current.temperature_2m}°F`);
}

// Test 3: GitHub Trending
console.log('\n⭐ Test 3: GitHub Trending Repositories');
const trending = await fetchGitHubTrending();
console.log(`✓ Found ${trending.length} trending repositories`);

// Test 4: News
console.log('\n📰 Test 4: News Search');
const news = await fetchNews('technology', 3);
console.log(`✓ Found ${news.length} tech news articles`);

// Test 5: Generate Charts
console.log('\n📊 Test 5: Chart Generation');

const salesData = [
  { label: 'Q1 2024', value: 45000 },
  { label: 'Q2 2024', value: 52000 },
  { label: 'Q3 2024', value: 48000 },
  { label: 'Q4 2024', value: 61000 }
];

const regionData = [
  { label: 'North America', value: 85000 },
  { label: 'Europe', value: 62000 },
  { label: 'Asia', value: 58000 },
  { label: 'South America', value: 31000 },
  { label: 'Africa', value: 24000 }
];

// Generate individual charts (suppress console output from Sharp)
try {
  const barChart = await generateBarChart(salesData, 'Quarterly Sales 2024', 'Quarter', 'Sales ($)');
  console.log(`✓ Bar chart generated: ${barChart.length} bytes`);
} catch (err) {
  console.warn(`⚠ Bar chart failed: ${err.message}`);
}

try {
  const pieChart = await generatePieChart(regionData, 'Revenue by Region');
  console.log(`✓ Pie chart generated: ${pieChart.length} bytes`);
} catch (err) {
  console.warn(`⚠ Pie chart failed: ${err.message}`);
}

try {
  const lineChart = await generateLineChart(salesData, 'Sales Trend', 'Period', 'Revenue');
  console.log(`✓ Line chart generated: ${lineChart.length} bytes`);
} catch (err) {
  console.warn(`⚠ Line chart failed: ${err.message}`);
}

try {
  const doughnutChart = await generateDoughnutChart(regionData, 'Market Share');
  console.log(`✓ Doughnut chart generated: ${doughnutChart.length} bytes`);
} catch (err) {
  console.warn(`⚠ Doughnut chart failed: ${err.message}`);
}

// Test 6: Images
console.log('\n🖼️ Test 6: Image Processing');
const testImages = [
  'https://via.placeholder.com/600x400/FF6B6B/FFFFFF?text=Dashboard',
  'https://via.placeholder.com/600x400/4ECDC4/FFFFFF?text=Analytics'
];

for (const imgUrl of testImages) {
  try {
    const img = await getImageForDocument(imgUrl, { width: 600, height: 400 });
    if (img) {
      console.log(`✓ Image downloaded and optimized: ${img.length} bytes`);
    }
  } catch (err) {
    console.warn(`⚠ Image failed: ${err.message}`);
  }
}

// Test 7: Excel with Charts
console.log('\n📈 Test 7: Excel with Charts');
const excelData = [
  { Quarter: 'Q1', Revenue: 45000, Expenses: 32000, Profit: 13000 },
  { Quarter: 'Q2', Revenue: 52000, Expenses: 35000, Profit: 17000 },
  { Quarter: 'Q3', Revenue: 48000, Expenses: 33000, Profit: 15000 },
  { Quarter: 'Q4', Revenue: 61000, Expenses: 40000, Profit: 21000 }
];

const chartConfigs = [
  { type: 'bar', data: salesData, title: 'Quarterly Sales' },
  { type: 'pie', data: regionData, title: 'Revenue Distribution' }
];

try {
  const excelWithCharts = await convertToExcelWithCharts(
    JSON.stringify(excelData),
    chartConfigs
  );
  fs.writeFileSync('output/professional-report.xlsx', excelWithCharts);
  console.log(`✓ Excel with charts created: ${excelWithCharts.length} bytes`);
} catch (err) {
  console.error(`✗ Excel generation failed: ${err.message}`);
}

// Test 8: PowerPoint with Visuals
console.log('\n🎨 Test 8: PowerPoint with Charts & Images');
try {
  const pptxWithVisuals = await convertToPptxWithVisuals(
    JSON.stringify(excelData),
    chartConfigs,
    testImages
  );
  
  if (pptxWithVisuals) {
    fs.writeFileSync('output/professional-presentation.pptx', pptxWithVisuals);
    console.log(`✓ PowerPoint with visuals created: ${pptxWithVisuals.length} bytes`);
  }
} catch (err) {
  console.error(`✗ PowerPoint generation failed: ${err.message}`);
}

// Test 9: PDF Report
console.log('\n📄 Test 9: PDF Report');
try {
  const pdfBuffer = await convertFile(
    JSON.stringify(excelData),
    'pdf',
    'professional-report'
  );
  fs.writeFileSync('output/professional-report.pdf', pdfBuffer);
  console.log(`✓ Professional PDF created: ${pdfBuffer.length} bytes`);
} catch (err) {
  console.error(`✗ PDF generation failed: ${err.message}`);
}

// Test 10: Multi-format Export
console.log('\n🔄 Test 10: Multi-format Export');
const formats = ['xlsx', 'pdf', 'docx', 'pptx', 'csv', 'json'];
for (const fmt of formats) {
  try {
    const buffer = await convertFile(JSON.stringify(excelData), fmt, `report.${fmt}`);
    if (buffer) {
      fs.writeFileSync(`output/report.${fmt}`, buffer);
      console.log(`✓ ${fmt.toUpperCase()}: ${buffer.length} bytes`);
    }
  } catch (err) {
    console.error(`✗ ${fmt}: ${err.message}`);
  }
}

console.log('\n✨ Professional Features Test Complete!\n');
console.log('Generated files in output/:');
const files = fs.readdirSync('output/').sort();
files.forEach(f => {
  const stat = fs.statSync(`output/${f}`);
  console.log(`  - ${f} (${stat.size} bytes)`);
});
