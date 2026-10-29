#!/bin/bash

# Exit on error
set -eo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}🚀 Starting deployment...${NC}"
git pull origin main

# Pull and start Docker containers
echo -e "${BLUE}🐳 Pulling and starting Docker containers...${NC}"
docker compose pull
docker compose up -d

# Wait for the application to be ready
echo -e "${BLUE}⏳ Waiting for application to be ready...${NC}"
status=1
for i in $(seq 1 10); do
    if curl -sfk https://alarmy.lan/api/health > /dev/null; then
        status=0
        break
    fi
    sleep 3
done

if [ $status -eq 0 ]; then
    echo -e "${GREEN}✅ Application is running successfully!${NC}"
    echo -e "${GREEN}🌐 Access the application at: https://alarmy.lan/${NC}"
else
    echo -e "${RED}❌ Application failed to start${NC}"
fi

echo -e "${BLUE}📝 Checking logs...${NC}"
docker compose logs

echo -e "${BLUE}🧹 Cleaning up old images...${NC}"
docker image prune -f

exit $status
 
