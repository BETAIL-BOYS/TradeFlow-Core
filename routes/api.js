const express = require("express");
const axios = require("axios");
const packageJson = require("../package.json");
const httpStatusCodes = require("../utils/httpStatusCodes");

const router = express.Router();

const priceCache = {};
const CACHE_DURATION = 60 * 1000;

async function fetchPrices() {
  try {
    const response = await axios.get(
      "https://api.coingecko.com/api/v3/simple/price?ids=bitcoin,ethereum&vs_currencies=usd,eur",
    );
    return response.data;
  } catch (error) {
    console.error("Error fetching prices:", error.message);
    throw error;
  }
}

// Prices endpoint
router.get("/prices", async (req, res) => {
  const now = Date.now();

  if (priceCache.data && now - priceCache.timestamp < CACHE_DURATION) {
    return res.json({
      ...priceCache.data,
      cached: true,
      timestamp: new Date(priceCache.timestamp).toISOString(),
    });
  }

  try {
    const prices = await fetchPrices();
    priceCache.data = prices;
    priceCache.timestamp = now;

    res.json({
      ...prices,
      cached: false,
      timestamp: new Date(now).toISOString(),
    });
  } catch (error) {
    res.status(httpStatusCodes.INTERNAL_SERVER_ERROR).json({
      error: "Failed to fetch prices",
      message: error.message,
    });
  }
});

// Test endpoint
router.get("/test", (req, res) => {
  res.json({ message: "Test endpoint working" });
});

// Version endpoint
router.get("/version", (req, res) => {
  res.json({ version: packageJson.version });
});

module.exports = router;
