#!/usr/bin/env node

// Professional Chart Generator
// Creates charts, graphs, pie charts, and other visualizations using SVG

import sharp from 'sharp';

/**
 * Create a pie chart as SVG string
 */
function generatePieChartSVG(data, title = 'Pie Chart') {
  const labels = data.map(item => item.label || item.name);
  const values = data.map(item => item.value || item.amount);
  const total = values.reduce((a, b) => a + b, 0);
  const colors = [
    '#FF6384', '#36A2EB', '#FFCE56', '#4BC0C0', '#9966FF',
    '#FF9F40', '#FF6384', '#C9CBCF', '#4BC0C0', '#FF6384'
  ];

  const width = 800;
  const height = 600;
  const centerX = width / 2;
  const centerY = height / 2;
  const radius = 150;

  let svg = `<svg width="${width}" height="${height}" xmlns="http://www.w3.org/2000/svg">
    <rect width="${width}" height="${height}" fill="white"/>
    <text x="${centerX}" y="40" font-size="24" font-weight="bold" text-anchor="middle">${title}</text>`;

  let currentAngle = 0;
  labels.forEach((label, idx) => {
    const value = values[idx];
    const sliceAngle = (value / total) * 360;
    const startAngle = currentAngle;
    const endAngle = currentAngle + sliceAngle;

    const x1 = centerX + radius * Math.cos((startAngle * Math.PI) / 180);
    const y1 = centerY + radius * Math.sin((startAngle * Math.PI) / 180);
    const x2 = centerX + radius * Math.cos((endAngle * Math.PI) / 180);
    const y2 = centerY + radius * Math.sin((endAngle * Math.PI) / 180);

    const largeArc = sliceAngle > 180 ? 1 : 0;

    svg += `
      <path d="M ${centerX} ${centerY} L ${x1} ${y1} A ${radius} ${radius} 0 ${largeArc} 1 ${x2} ${y2} Z" 
            fill="${colors[idx % colors.length]}" stroke="white" stroke-width="2"/>`;

    currentAngle = endAngle;
  });

  // Add legend
  let legendY = 100;
  labels.forEach((label, idx) => {
    svg += `
      <rect x="100" y="${legendY}" width="15" height="15" fill="${colors[idx % colors.length]}"/>
      <text x="125" y="${legendY + 12}" font-size="12">${label}</text>`;
    legendY += 25;
  });

  svg += '</svg>';
  return svg;
}

/**
 * Create a bar chart as SVG string
 */
function generateBarChartSVG(data, title = 'Bar Chart', xLabel = 'Category', yLabel = 'Value') {
  const labels = data.map(item => item.label || item.name || item.category);
  const values = data.map(item => item.value || item.amount);
  const maxValue = Math.max(...values);

  const width = 800;
  const height = 600;
  const chartLeft = 80;
  const chartTop = 60;
  const chartWidth = width - chartLeft - 50;
  const chartHeight = height - chartTop - 100;

  let svg = `<svg width="${width}" height="${height}" xmlns="http://www.w3.org/2000/svg">
    <rect width="${width}" height="${height}" fill="white"/>
    <text x="${width / 2}" y="35" font-size="24" font-weight="bold" text-anchor="middle">${title}</text>`;

  // Y axis
  svg += `<line x1="${chartLeft}" y1="${chartTop}" x2="${chartLeft}" y2="${chartTop + chartHeight}" stroke="black" stroke-width="2"/>`;
  
  // X axis
  svg += `<line x1="${chartLeft}" y1="${chartTop + chartHeight}" x2="${chartLeft + chartWidth}" y2="${chartTop + chartHeight}" stroke="black" stroke-width="2"/>`;

  const barWidth = chartWidth / (labels.length * 1.5);
  const barGap = barWidth * 0.5;

  values.forEach((value, idx) => {
    const barHeight = (value / maxValue) * chartHeight;
    const x = chartLeft + idx * (barWidth + barGap) + barGap / 2;
    const y = chartTop + chartHeight - barHeight;

    svg += `
      <rect x="${x}" y="${y}" width="${barWidth}" height="${barHeight}" fill="#36A2EB" stroke="#1E40AF" stroke-width="1"/>
      <text x="${x + barWidth / 2}" y="${chartTop + chartHeight + 20}" font-size="11" text-anchor="middle">${labels[idx]}</text>
      <text x="${x + barWidth / 2}" y="${y - 5}" font-size="10" text-anchor="middle" font-weight="bold">${value}</text>`;
  });

  // Y axis label
  svg += `<text x="20" y="${chartTop + chartHeight / 2}" font-size="12" text-anchor="middle" transform="rotate(-90 20 ${chartTop + chartHeight / 2})">${yLabel}</text>`;
  
  // X axis label
  svg += `<text x="${chartLeft + chartWidth / 2}" y="${chartTop + chartHeight + 60}" font-size="12" text-anchor="middle">${xLabel}</text>`;

  svg += '</svg>';
  return svg;
}

/**
 * Create a line chart as SVG string
 */
function generateLineChartSVG(data, title = 'Line Chart', xLabel = 'Time', yLabel = 'Value') {
  const labels = data.map(item => item.label || item.name || item.period);
  const values = data.map(item => item.value || item.amount);
  const maxValue = Math.max(...values);
  const minValue = Math.min(...values);

  const width = 800;
  const height = 600;
  const chartLeft = 80;
  const chartTop = 60;
  const chartWidth = width - chartLeft - 50;
  const chartHeight = height - chartTop - 100;

  let svg = `<svg width="${width}" height="${height}" xmlns="http://www.w3.org/2000/svg">
    <rect width="${width}" height="${height}" fill="white"/>
    <text x="${width / 2}" y="35" font-size="24" font-weight="bold" text-anchor="middle">${title}</text>`;

  // Y axis
  svg += `<line x1="${chartLeft}" y1="${chartTop}" x2="${chartLeft}" y2="${chartTop + chartHeight}" stroke="black" stroke-width="2"/>`;
  
  // X axis
  svg += `<line x1="${chartLeft}" y1="${chartTop + chartHeight}" x2="${chartLeft + chartWidth}" y2="${chartTop + chartHeight}" stroke="black" stroke-width="2"/>`;

  const range = maxValue - minValue || 1;
  const pointSpacing = chartWidth / (labels.length - 1 || 1);

  // Draw line
  let pathData = '';
  values.forEach((value, idx) => {
    const x = chartLeft + idx * pointSpacing;
    const y = chartTop + chartHeight - ((value - minValue) / range) * chartHeight;
    pathData += (idx === 0 ? 'M' : 'L') + ` ${x} ${y}`;
  });

  svg += `<path d="${pathData}" stroke="#FF6384" stroke-width="3" fill="none"/>`;

  // Draw points and labels
  values.forEach((value, idx) => {
    const x = chartLeft + idx * pointSpacing;
    const y = chartTop + chartHeight - ((value - minValue) / range) * chartHeight;

    svg += `
      <circle cx="${x}" cy="${y}" r="5" fill="#FF6384" stroke="white" stroke-width="2"/>
      <text x="${x}" y="${chartTop + chartHeight + 20}" font-size="11" text-anchor="middle">${labels[idx]}</text>
      <text x="${x}" y="${y - 10}" font-size="10" text-anchor="middle" font-weight="bold">${value}</text>`;
  });

  svg += '</svg>';
  return svg;
}

/**
 * Create a doughnut chart as SVG string
 */
function generateDoughnutChartSVG(data, title = 'Doughnut Chart') {
  return generatePieChartSVG(data, title); // For now, same as pie
}

/**
 * Convert SVG to PNG buffer
 */
async function svgToPNG(svgString) {
  try {
    return await sharp(Buffer.from(svgString)).png().toBuffer();
  } catch (error) {
    console.error('Failed to convert SVG to PNG:', error.message);
    return null;
  }
}

/**
 * Generate pie chart
 */
async function generatePieChart(data, title = 'Pie Chart') {
  const svg = generatePieChartSVG(data, title);
  return await svgToPNG(svg);
}

/**
 * Generate bar chart
 */
async function generateBarChart(data, title = 'Bar Chart', xAxisLabel = 'Category', yAxisLabel = 'Value') {
  const svg = generateBarChartSVG(data, title, xAxisLabel, yAxisLabel);
  return await svgToPNG(svg);
}

/**
 * Generate line chart
 */
async function generateLineChart(data, title = 'Line Chart', xAxisLabel = 'Time', yAxisLabel = 'Value') {
  const svg = generateLineChartSVG(data, title, xAxisLabel, yAxisLabel);
  return await svgToPNG(svg);
}

/**
 * Generate doughnut chart
 */
async function generateDoughnutChart(data, title = 'Doughnut Chart') {
  const svg = generateDoughnutChartSVG(data, title);
  return await svgToPNG(svg);
}

/**
 * Generate comparison chart
 */
async function generateComparisonChart(datasets, title = 'Comparison Chart', chartType = 'bar') {
  // For comparison, create a multi-series bar chart
  if (chartType === 'bar' && datasets.length > 0) {
    const data = [];
    datasets.forEach((ds, dsIdx) => {
      ds.data.forEach((item, idx) => {
        if (!data[idx]) data[idx] = { label: item.label || item.name, series: [] };
        data[idx].series.push(item.value || item.amount);
      });
    });

    // Create a simple bar chart with data
    return await generateBarChart(data, title);
  }

  return await generateBarChart(datasets[0].data, title);
}

export {
  generatePieChart,
  generateBarChart,
  generateLineChart,
  generateComparisonChart,
  generateDoughnutChart
};
