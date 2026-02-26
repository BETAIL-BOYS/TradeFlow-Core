const express = require("express");
const rateLimit = require("express-rate-limit");
require("dotenv").config();

const apiRoutes = require("./routes/api");

function logWithTime(message) {
  console.log(`[${new Date().toISOString()}] ${message}`);
}

const app = express();
const PORT = process.env.PORT || 3000;

const limiter = rateLimit({
  windowMs: 15 * 60 * 1000,
  max: 100,
  message: {
    error: "Too many requests from this IP, please try again after 15 minutes.",
  },
  standardHeaders: true,
  legacyHeaders: false,
});

app.use(express.json());

// Health route stays in server (bootstrap concern)
app.get("/health", (req, res) => {
  res.json({
    status: "OK",
    timestamp: new Date().toISOString(),
  });
});

// Apply rate limit to API
app.use("/api/v1", limiter, apiRoutes);

app.listen(PORT, () => {
  logWithTime(`Server running on port ${PORT}`);
  logWithTime(`Health check available at: http://localhost:${PORT}/health`);
});
