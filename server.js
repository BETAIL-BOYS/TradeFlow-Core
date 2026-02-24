const express = require('express');

require('dotenv').config();

const app = express();
const PORT = process.env.PORT || 3000;


    timestamp: new Date().toISOString()
  });
});


  console.log(`Health check available at: http://localhost:${PORT}/health`);
});
