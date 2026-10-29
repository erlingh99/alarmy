#!/bin/bash

# Exit on error
set -eo pipefail

# Colors for output
BLUE='\033[0;34m'
GREEN='\033[0;32m'
NC='\033[0m' # No Color

echo -e "${BLUE}🔄 Restarting application...${NC}"

# Stop the application
./scripts/stop.sh

# Start the application
./scripts/deploy.sh

echo -e "${GREEN}✅ Application restarted successfully${NC}" 
