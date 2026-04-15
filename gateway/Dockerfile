# x402 Gateway — TypeScript Express app
# See spec Section 16.2 for deployment architecture.

FROM node:22-slim
WORKDIR /app
COPY package.json ./
RUN npm install --production
COPY dist/ dist/
EXPOSE 443
CMD ["node", "dist/index.js"]
