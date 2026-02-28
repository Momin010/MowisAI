#!/usr/bin/env node

// Image Handler
// Downloads images, processes them, and embeds them in documents

import axios from 'axios';
import sharp from 'sharp';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const imageCache = path.join(__dirname, '.image_cache');

// Create cache directory if it doesn't exist
if (!fs.existsSync(imageCache)) {
  fs.mkdirSync(imageCache, { recursive: true });
}

/**
 * Download image from URL with caching
 */
async function downloadImage(url) {
  try {
    // Create cache key from URL
    const cacheKey = Buffer.from(url).toString('base64').replace(/[^a-zA-Z0-9]/g, '');
    const cachedPath = path.join(imageCache, cacheKey);

    // Return cached image if exists
    if (fs.existsSync(cachedPath)) {
      return fs.readFileSync(cachedPath);
    }

    // Download image
    const response = await axios.get(url, {
      responseType: 'arraybuffer',
      timeout: 10000,
      headers: {
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
      }
    });

    const imageBuffer = Buffer.from(response.data);

    // Cache the image
    fs.writeFileSync(cachedPath, imageBuffer);

    return imageBuffer;
  } catch (error) {
    console.error(`Failed to download image from ${url}:`, error.message);
    return null;
  }
}

/**
 * Resize image to specific dimensions
 */
async function resizeImage(imageBuffer, width, height) {
  try {
    return await sharp(imageBuffer)
      .resize(width, height, { fit: 'cover', position: 'center' })
      .toBuffer();
  } catch (error) {
    console.error('Failed to resize image:', error.message);
    return imageBuffer;
  }
}

/**
 * Optimize image for web/documents
 */
async function optimizeImage(imageBuffer, maxWidth = 1200) {
  try {
    const metadata = await sharp(imageBuffer).metadata();
    
    if (metadata.width > maxWidth) {
      return await sharp(imageBuffer)
        .resize(maxWidth, Math.round((metadata.height * maxWidth) / metadata.width))
        .toBuffer();
    }
    
    return imageBuffer;
  } catch (error) {
    console.error('Failed to optimize image:', error.message);
    return imageBuffer;
  }
}

/**
 * Convert image to PNG format
 */
async function convertToPNG(imageBuffer) {
  try {
    return await sharp(imageBuffer).png().toBuffer();
  } catch (error) {
    console.error('Failed to convert image to PNG:', error.message);
    return imageBuffer;
  }
}

/**
 * Create a gradient background
 */
async function createGradient(width, height, color1, color2) {
  try {
    // Create SVG gradient
    const svg = `
      <svg width="${width}" height="${height}" xmlns="http://www.w3.org/2000/svg">
        <defs>
          <linearGradient id="grad" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" style="stop-color:${color1};stop-opacity:1" />
            <stop offset="100%" style="stop-color:${color2};stop-opacity:1" />
          </linearGradient>
        </defs>
        <rect width="${width}" height="${height}" fill="url(#grad)"/>
      </svg>
    `;
    
    return await sharp(Buffer.from(svg)).png().toBuffer();
  } catch (error) {
    console.error('Failed to create gradient:', error.message);
    return null;
  }
}

/**
 * Add text overlay to image
 */
async function addTextOverlay(imageBuffer, text, options = {}) {
  try {
    const {
      fontSize = 48,
      fontColor = '#FFFFFF',
      position = 'center',
      fontFamily = 'sans-serif'
    } = options;

    // Create SVG text overlay
    const metadata = await sharp(imageBuffer).metadata();
    const svg = `
      <svg width="${metadata.width}" height="${metadata.height}" xmlns="http://www.w3.org/2000/svg">
        <text x="50%" y="50%" 
              font-size="${fontSize}" 
              font-family="${fontFamily}"
              fill="${fontColor}"
              text-anchor="middle"
              dominant-baseline="middle"
              font-weight="bold">
          ${text}
        </text>
      </svg>
    `;

    return await sharp(imageBuffer)
      .composite([{
        input: Buffer.from(svg),
        top: 0,
        left: 0
      }])
      .toBuffer();
  } catch (error) {
    console.error('Failed to add text overlay:', error.message);
    return imageBuffer;
  }
}

/**
 * Fetch image from URL for document embedding
 */
async function getImageForDocument(url, options = {}) {
  try {
    const {
      width = 600,
      height = 400,
      optimize = true
    } = options;

    // Download image
    let imageBuffer = await downloadImage(url);
    if (!imageBuffer) return null;

    // Optimize if requested
    if (optimize) {
      imageBuffer = await optimizeImage(imageBuffer, width);
    }

    // Resize to document dimensions
    imageBuffer = await resizeImage(imageBuffer, width, height);

    // Convert to PNG for consistency
    imageBuffer = await convertToPNG(imageBuffer);

    return imageBuffer;
  } catch (error) {
    console.error('Failed to get image for document:', error.message);
    return null;
  }
}

/**
 * Process multiple images
 */
async function processImages(imageUrls, options = {}) {
  const results = [];
  
  for (const url of imageUrls) {
    const image = await getImageForDocument(url, options);
    if (image) {
      results.push({
        url,
        buffer: image,
        size: image.length
      });
    }
  }

  return results;
}

/**
 * Create a collage from multiple images
 */
async function createCollage(imageUrls, columns = 3, imageWidth = 300, imageHeight = 200) {
  try {
    const images = await processImages(imageUrls, {
      width: imageWidth,
      height: imageHeight
    });

    if (images.length === 0) return null;

    const rows = Math.ceil(images.length / columns);
    const collageWidth = columns * imageWidth;
    const collageHeight = rows * imageHeight;

    let composite = await sharp({
      create: {
        width: collageWidth,
        height: collageHeight,
        channels: 3,
        background: { r: 240, g: 240, b: 240 }
      }
    });

    const compositeArray = images.map((img, idx) => ({
      input: img.buffer,
      top: Math.floor(idx / columns) * imageHeight,
      left: (idx % columns) * imageWidth
    }));

    return await composite.composite(compositeArray).png().toBuffer();
  } catch (error) {
    console.error('Failed to create collage:', error.message);
    return null;
  }
}

export {
  downloadImage,
  resizeImage,
  optimizeImage,
  convertToPNG,
  createGradient,
  addTextOverlay,
  getImageForDocument,
  processImages,
  createCollage
};
